use std::collections::HashSet;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use chrono::Utc;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use crate::api::routes::AppState;
use crate::models::protocol::{ClientCommand, LogEntry, ServerEvent, StreamChannel};

/// GET /api/ws - WebSocket 握手升級端點
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // 為此 WebSocket 連線申請一個獨立的廣播接收器
    let mut event_rx = state.scheduler.subscribe();

    // 紀錄該前端連線目前正在監看 (開啟終端機面板) 的 Task ID 集合
    let mut subscribed_tasks: HashSet<Uuid> = HashSet::new();

    // 連線建立成功後，立即主動推送一次當前系統佇列指標
    state.scheduler.broadcast_metrics();

    loop {
        tokio::select! {
            // ==========================================
            // 分支 A：接收來自 TaskScheduler 的後端廣播事件
            // ==========================================
            broadcast_res = event_rx.recv() => {
                match broadcast_res {
                    Ok(event) => {
                        if should_forward_event(&event, &subscribed_tasks) {
                            if let Ok(json_text) = serde_json::to_string(&event) {
                                if socket.send(Message::Text(json_text.into())).await.is_err() {
                                    // 客戶端已斷線
                                    break;
                                }
                            }
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        // 當前端網路過慢導致 Ring Buffer 覆寫時，通知前端有多少封包被略過
                        let warn_event = ServerEvent::Error {
                            task_id: None,
                            code: "STREAM_LAGGED".to_string(),
                            message: format!("前端接收緩慢，已略過 {} 個歷史串流封包", skipped),
                        };
                        if let Ok(json_text) = serde_json::to_string(&warn_event) {
                            let _ = socket.send(Message::Text(json_text.into())).await;
                        }
                    }
                    Err(RecvError::Closed) => {
                        // 伺服器正在關閉
                        break;
                    }
                }
            }

            // ==========================================
            // 分支 B：接收並解析來自 Vue 前端的控制指令
            // ==========================================
            maybe_msg = socket.recv() => {
                match maybe_msg {
                    Some(Ok(Message::Text(raw_text))) => {
                        match serde_json::from_str::<ClientCommand>(&raw_text) {
                            Ok(cmd) => {
                                handle_client_command(
                                    cmd,
                                    &state,
                                    &mut subscribed_tasks,
                                    &mut socket,
                                ).await;
                            }
                            Err(e) => {
                                let err_event = ServerEvent::Error {
                                    task_id: None,
                                    code: "INVALID_COMMAND_FORMAT".to_string(),
                                    message: format!("無法解析 ClientCommand JSON: {}", e),
                                };
                                if let Ok(json) = serde_json::to_string(&err_event) {
                                    let _ = socket.send(Message::Text(json.into())).await;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                        // 瀏覽器關閉分頁或連線中斷
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 處理前端透過 WebSocket 傳來的指令
async fn handle_client_command(
    cmd: ClientCommand,
    state: &AppState,
    subscribed_tasks: &mut HashSet<Uuid>,
    socket: &mut WebSocket,
) {
    match cmd {
        ClientCommand::SubscribeTask { task_id } => {
            subscribed_tasks.insert(task_id);

            // 回傳一筆系統確認 Log 讓前端 Xterm.js 顯示已成功掛載串流
            let ack_event = ServerEvent::StreamBatch {
                task_id,
                logs: vec![LogEntry {
                    seq: 0,
                    channel: StreamChannel::System,
                    line: format!("已連接至任務 [{}] 即時輸出串流...", task_id),
                    timestamp: Utc::now(),
                }],
            };
            if let Ok(json) = serde_json::to_string(&ack_event) {
                let _ = socket.send(Message::Text(json.into())).await;
            }
        }

        ClientCommand::UnsubscribeTask { task_id } => {
            subscribed_tasks.remove(&task_id);
        }

        ClientCommand::AbortTask { task_id } => {
            let success = state.scheduler.abort_task(task_id);
            if !success {
                let err_event = ServerEvent::Error {
                    task_id: Some(task_id),
                    code: "ABORT_FAILED".to_string(),
                    message: "找不到該活躍任務，可能已經執行完畢或已被取消。".to_string(),
                };
                if let Ok(json) = serde_json::to_string(&err_event) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
        }

        ClientCommand::Ping => {
            // 收到應用層 PING 時，順便回推最新的 SystemMetrics 作為 Heartbeat 回應
            state.scheduler.broadcast_metrics();
        }
    }
}

/// 決定是否將廣播事件轉發給此 WebSocket 連線
fn should_forward_event(event: &ServerEvent, subscribed: &HashSet<Uuid>) -> bool {
    match event {
        // 高頻率的終端機輸出，僅在前端明確訂閱該 task_id 時才轉發
        ServerEvent::StreamBatch { task_id, .. } => subscribed.contains(task_id),

        // 任務狀態改變、結構化產出就緒、系統負載與錯誤，皆屬於全域控制事件，一律推送
        ServerEvent::TaskStateChanged { .. }
        | ServerEvent::ArtifactReady { .. }
        | ServerEvent::SystemMetrics { .. }
        | ServerEvent::Error { .. } => true,
    }
}

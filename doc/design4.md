這份實作將 `TaskScheduler` 封裝進全域共享的 `AppState`，並利用 Axum 的 Extractor 機制處理 RESTful 請求與 WebSocket 雙向連線。

在 WebSocket 層（`api/ws.rs`）中，我們實作了「智慧型頻道過濾器（Subscription Filter）」：雖然後端的 `broadcast` 頻道會廣播所有任務的事件，但每個 WebSocket 連線在伺服器端都維護一個獨立的 `HashSet<Uuid>` 訂閱清單。只有當前端發送 `SUBSCRIBE_TASK` 訂閱特定任務時，該任務的高頻率終端機串流（`STREAM_BATCH`）才會被推送到該瀏覽器連線，避免多個任務並行時塞爆前端頻寬；而全域狀態變更（`TASK_STATE_CHANGED`、`SYSTEM_METRICS`）則會無條件推送，確保前端側邊欄的任務列表隨時保持最新狀態。

在 `Cargo.toml` 中需確保 `axum` 啟用了 `ws` 與 `macros` features，並引入 `tower-http`（啟用 `cors`）與 `tracing`。

---

## 1. 共享狀態與 REST 路由：`backend-rust/src/api/routes.rs`

此模組負責維護任務的記憶體快取清單（`DashMap<Uuid, TaskMetadata>`），提供建立任務、查詢任務列表與強制中止任務的 HTTP 端點，同時啟動一個背景同步協程，自動根據廣播事件更新任務的最終狀態與完成時間。

```rust
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use dashmap::DashMap;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::api::ws::ws_handler;
use crate::executor::scheduler::TaskScheduler;
use crate::models::protocol::{CreateTaskRequest, ServerEvent, TaskMetadata, TaskState};

/// 掛載於 Axum Router 的全域共享狀態
#[derive(Clone)]
pub struct AppState {
    pub scheduler: Arc<TaskScheduler>,
    /// 儲存所有任務的元資料，供前端重新整理頁面時透過 GET /api/tasks 載入
    pub tasks: Arc<DashMap<Uuid, TaskMetadata>>,
}

impl AppState {
    pub fn new(scheduler: TaskScheduler) -> Self {
        let scheduler = Arc::new(scheduler);
        let tasks: Arc<DashMap<Uuid, TaskMetadata>> = Arc::new(DashMap::new());

        // 啟動背景 Listener，自動將廣播中的狀態變更同步回 tasks Map
        let mut rx = scheduler.subscribe();
        let tasks_clone = tasks.clone();

        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if let ServerEvent::TaskStateChanged {
                    task_id,
                    new_state,
                    exit_code,
                    timestamp,
                    ..
                } = event
                {
                    if let Some(mut task) = tasks_clone.get_mut(&task_id) {
                        task.state = new_state;
                        if exit_code.is_some() {
                            task.exit_code = exit_code;
                        }
                        match new_state {
                            TaskState::Running => task.started_at = Some(timestamp),
                            TaskState::Completed
                            | TaskState::Failed
                            | TaskState::Cancelled
                            | TaskState::Timeout => {
                                task.finished_at = Some(timestamp);
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Self { scheduler, tasks }
    }
}

/// 建構完整的 Axum Router
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any) // 生產環境建議改為前端實際 Origin (如 http://localhost:5173)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // RESTful API 路由
        .route("/api/tasks", post(create_task_handler).get(list_tasks_handler))
        .route("/api/tasks/{id}", get(get_task_handler))
        .route("/api/tasks/{id}/cancel", delete(cancel_task_handler))
        // WebSocket 即時串流路由
        .route("/api/ws", get(ws_handler))
        .layer(cors)
        .with_state(state)
}

/// POST /api/tasks - 派發新的 Antigravity CLI 任務
async fn create_task_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskMetadata>), (StatusCode, String)> {
    if req.prompt.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Prompt 內容不得為空".to_string()));
    }

    let metadata = state.scheduler.enqueue(req);
    state.tasks.insert(metadata.task_id, metadata.clone());

    Ok((StatusCode::CREATED, Json(metadata)))
}

/// GET /api/tasks - 取得所有歷史與執行中任務清單（依建立時間倒序排列）
async fn list_tasks_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mut list: Vec<TaskMetadata> = state.tasks.iter().map(|entry| entry.value().clone()).collect();
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Json(list)
}

/// GET /api/tasks/{id} - 查詢單一任務狀態
async fn get_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<TaskMetadata>, StatusCode> {
    state
        .tasks
        .get(&task_id)
        .map(|t| Json(t.value().clone()))
        .ok_or(StatusCode::NOT_FOUND)
}

/// DELETE /api/tasks/{id}/cancel - 透過 HTTP 強制中止排隊中或執行中的任務
async fn cancel_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> impl IntoResponse {
    if state.scheduler.abort_task(task_id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

```

---

## 2. WebSocket 雙向多工與訂閱過濾：`backend-rust/src/api/ws.rs`

此模組利用 `tokio::select!` 在單一非同步迴圈中同時處理：

1. **來自前端的指令**（`SUBSCRIBE_TASK`、`UNSUBSCRIBE_TASK`、`ABORT_TASK`、`PING`）。
2. **來自 `TaskScheduler` 的廣播事件**，並根據當前連線的 `subscribed_tasks` 集合決定是否轉發給前端。
3. **廣播通道落後補償（Lagged Handling）**：當某個前端連線網路卡頓導致 `broadcast::Receiver` 發生溢位（`RecvError::Lagged`）時，自動向前端發送系統警告而不中斷連線。

```rust
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

```

---

## 3. 伺服器啟動入口：`backend-rust/src/main.rs`

將 `TaskScheduler`、`AppState` 與 Axum `Router` 組裝起來並綁定於本地端點：

```rust
mod api;
mod executor;
mod models;

use std::net::SocketAddr;
use std::path::PathBuf;

use api::routes::{create_router, AppState};
use executor::scheduler::TaskScheduler;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // 1. 設定允許 CLI 存取的根工作目錄 (例如當前使用者家目錄下的 projects 資料夾)
    let base_workspace = std::env::var("ANTIGRAVITY_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().expect("無法取得當前目錄"));

    // 2. 初始化並行排程器 (例如限制最多同時執行 2 個 Antigravity CLI 行程)
    let max_concurrency = 2;
    let (scheduler, _initial_rx) = TaskScheduler::new(max_concurrency, base_workspace.clone());

    // 3. 建立共享狀態與 Axum 路由
    let app_state = AppState::new(scheduler);
    let app = create_router(app_state);

    // 4. 預設綁定於 127.0.0.1:3000 確保本機 Token 安全
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("🚀 Antigravity Web-CLI Bridge 啟動於 http://{}", addr);
    println!("📂 允許的基準工作目錄: {:?}", base_workspace);

    let listener = TcpListener::bind(addr).await.expect("無法綁定連接埠 3000");
    axum::serve(listener, app).await.expect("伺服器非預期終止");
}

```

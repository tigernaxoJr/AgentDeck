這兩個核心模組利用 Tokio 的非同步多工與訊號處理機制，實現「安全隔離執行」、「50ms 背壓緩衝（Backpressure Batching）」與「精準的並行控制」。

在 `Cargo.toml` 中需確保引入 `tokio`（啟用 `full` features）與 `dashmap`（用於執行緒安全的活躍任務中止控制）。

---

## 1. 子行程執行與 50ms 視窗緩衝：`backend-rust/src/executor/runner.rs`

`runner.rs` 負責實際啟動 Antigravity CLI、攔截標準輸出/錯誤管道、實作 50ms 批次打包邏輯，並同時監聽「超時（Timeout）」與「使用者強制中止（Abort）」訊號。

```rust
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{interval, timeout, MissedTickBehavior};
use uuid::Uuid;

use crate::models::protocol::{
    ArtifactData, ArtifactType, CreateTaskRequest, LogEntry, ServerEvent, StreamChannel, TaskState,
};

const SUMMARY_REL_PATH: &str = ".antigravity_out/summary.json";

pub struct TaskRunner {
    pub task_id: Uuid,
    pub req: CreateTaskRequest,
    pub base_workspace: PathBuf,
    pub event_tx: broadcast::Sender<ServerEvent>,
}

impl TaskRunner {
    /// 執行 CLI 任務，傳入 abort_rx 以支援執行期強制中止
    pub async fn run(self, mut abort_rx: oneshot::Receiver<()>) -> TaskState {
        // 1. 驗證並解析安全的工作目錄 (防止 Path Traversal)
        let work_dir = match self.resolve_safe_workspace() {
            Ok(dir) => dir,
            Err(err_msg) => {
                self.emit_system_log(format!("Workspace 驗證失敗: {}", err_msg));
                self.emit_error("INVALID_WORKSPACE", &err_msg);
                return TaskState::Failed;
            }
        };

        // 2. 準備結構化輸出目錄與附加系統 Prompt
        let final_prompt = self.prepare_prompt_and_output_dir(&work_dir).await;

        // 3. 建構安全的非互動式 Command (杜絕 Shell Injection)
        let mut cmd = Command::new("antigravity");
        cmd.current_dir(&work_dir)
            .arg("run")
            .arg("--no-interactive") // 強制 Headless 模式，避免等待輸入卡死
            .arg("--yes")
            .args(&self.req.extra_flags)
            .arg("--prompt")
            .arg(&final_prompt)
            .stdin(Stdio::null())    // 切斷標準輸入，防止 Zombie Process
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);     // 當 Task 被 Drop 時自動由 OS 終止子行程

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("無法啟動 Antigravity CLI 行程: {}", e);
                self.emit_system_log(msg.clone());
                self.emit_error("SPAWN_FAILED", &msg);
                return TaskState::Failed;
            }
        };

        self.emit_system_log(format!(
            "PID {} 已啟動，工作目錄: {:?}",
            child.id().unwrap_or(0),
            work_dir
        ));

        // 4. 啟動 stdout / stderr 非同步讀取與 50ms 批次聚合器
        let stdout = child.stdout.take().expect("Failed to open stdout");
        let stderr = child.stderr.take().expect("Failed to open stderr");

        let (log_tx, log_rx) = mpsc::channel::<(StreamChannel, String)>(2048);
        let batcher_handle = tokio::spawn(Self::run_log_batcher(
            self.task_id,
            log_rx,
            self.event_tx.clone(),
        ));

        // 分別啟動 stdout 與 stderr 的讀取 Task
        let tx_out = log_tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx_out.send((StreamChannel::Stdout, line)).await.is_err() {
                    break;
                }
            }
        });

        let tx_err = log_tx.clone();
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx_err.send((StreamChannel::Stderr, line)).await.is_err() {
                    break;
                }
            }
        });

        // 釋放主執行緒持有的 Sender，確保當 stdout_task 與 stderr_task 結束時，log_rx 會收到 EOF
        drop(log_tx);

        // 5. 競爭等待：正常完成 vs 超時 (Timeout) vs 使用者中止 (Abort)
        let timeout_duration = Duration::from_secs(self.req.timeout_secs);
        let (final_state, exit_code) = tokio::select! {
            res = timeout(timeout_duration, child.wait()) => {
                match res {
                    Ok(Ok(status)) => {
                        let code = status.code();
                        let state = if status.success() { TaskState::Completed } else { TaskState::Failed };
                        (state, code)
                    }
                    Ok(Err(e)) => {
                        self.emit_system_log(format!("等待子行程時發生 I/O 錯誤: {}", e));
                        (TaskState::Failed, None)
                    }
                    Err(_) => {
                        // 觸發 Timeout
                        let _ = child.kill().await;
                        self.emit_system_log(format!("任務執行超過 {} 秒，已強制終止 (SIGKILL)。", self.req.timeout_secs));
                        (TaskState::Timeout, None)
                    }
                }
            }
            _ = &mut abort_rx => {
                // 使用者透過 WebSocket 發送 ABORT_TASK
                let _ = child.kill().await;
                self.emit_system_log("收到使用者中止指令，已強制終止 CLI 行程。".to_string());
                (TaskState::Cancelled, None)
            }
        };

        // 6. 確保所有殘留的管道輸出都被讀取完畢並推送至前端
        let _ = tokio::join!(stdout_task, stderr_task);
        let _ = batcher_handle.await;

        // 7. 若任務完成且要求結構化摘要，嘗試讀取並廣播 Artifact
        if final_state == TaskState::Completed && self.req.require_json_summary {
            self.try_harvest_artifact(&work_dir).await;
        }

        // 8. 廣播最終狀態轉換
        let _ = self.event_tx.send(ServerEvent::TaskStateChanged {
            task_id: self.task_id,
            old_state: Some(TaskState::Running),
            new_state: final_state,
            exit_code,
            timestamp: Utc::now(),
        });

        final_state
    }

    /// 50ms 視窗緩衝聚合器 (Throttling & Batching)
    async fn run_log_batcher(
        task_id: Uuid,
        mut rx: mpsc::Receiver<(StreamChannel, String)>,
        event_tx: broadcast::Sender<ServerEvent>,
    ) {
        let mut ticker = interval(Duration::from_millis(50));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut buffer: Vec<LogEntry> = Vec::with_capacity(128);
        let seq_counter = AtomicU64::new(1);

        loop {
            tokio::select! {
                maybe_item = rx.recv() => {
                    match maybe_item {
                        Some((channel, line)) => {
                            buffer.push(LogEntry {
                                seq: seq_counter.fetch_add(1, Ordering::Relaxed),
                                channel,
                                line,
                                timestamp: Utc::now(),
                            });
                            // 防護機制：若 50ms 內瞬間湧入超過 250 行，立即強制 Flush 避免記憶體暴增
                            if buffer.len() >= 250 {
                                let logs = std::mem::replace(&mut buffer, Vec::with_capacity(128));
                                let _ = event_tx.send(ServerEvent::StreamBatch { task_id, logs });
                            }
                        }
                        None => {
                            // 管道已關閉 (子行程結束)，將最後殘留的 Buffer 推出後退出迴圈
                            if !buffer.is_empty() {
                                let _ = event_tx.send(ServerEvent::StreamBatch { task_id, logs: buffer });
                            }
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        let logs = std::mem::replace(&mut buffer, Vec::with_capacity(128));
                        let _ = event_tx.send(ServerEvent::StreamBatch { task_id, logs });
                    }
                }
            }
        }
    }

    /// 驗證工作目錄是否合法且位於允許的 Base Workspace 之下
    fn resolve_safe_workspace(&self) -> Result<PathBuf, String> {
        let base = std::fs::canonicalize(&self.base_workspace)
            .map_err(|e| format!("系統基準目錄無效: {}", e))?;

        let target_raw = base.join(&self.req.workspace_path);
        let target = std::fs::canonicalize(&target_raw)
            .map_err(|e| format!("指定的目標目錄不存在或無法存取 ({:?}): {}", target_raw, e))?;

        if !target.starts_with(&base) {
            return Err("偵測到越權路徑存取 (Path Traversal Blocked)".to_string());
        }
        Ok(target)
    }

    /// 建立 `.antigravity_out` 並附加結構化輸出指令
    async fn prepare_prompt_and_output_dir(&self, work_dir: &Path) -> String {
        if !self.req.require_json_summary {
            return self.req.prompt.clone();
        }

        let out_file = work_dir.join(SUMMARY_REL_PATH);
        if let Some(parent) = out_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        // 清除前次殘留的檔案
        let _ = tokio::fs::remove_file(&out_file).await;

        format!(
            "{}\n\n[SYSTEM INSTRUCTION]: Upon completing all actions, write a structured JSON summary (including \"status\", \"summary\", and \"modified_files\") to `{}`.",
            self.req.prompt, SUMMARY_REL_PATH
        )
    }

    /// 檢查並解析 CLI 產出的結構化結果檔
    async fn try_harvest_artifact(&self, work_dir: &Path) {
        let out_file = work_dir.join(SUMMARY_REL_PATH);
        if let Ok(raw_text) = tokio::fs::read_to_string(&out_file).await {
            let (artifact_type, content) = match serde_json::from_str::<serde_json::Value>(&raw_text) {
                Ok(json_val) => (ArtifactType::Json, json_val),
                Err(_) => (ArtifactType::PlainText, serde_json::Value::String(raw_text)),
            };

            let _ = self.event_tx.send(ServerEvent::ArtifactReady {
                task_id: self.task_id,
                artifact: ArtifactData {
                    file_name: SUMMARY_REL_PATH.to_string(),
                    artifact_type,
                    content,
                },
            });
        }
    }

    fn emit_system_log(&self, message: String) {
        let _ = self.event_tx.send(ServerEvent::StreamBatch {
            task_id: self.task_id,
            logs: vec![LogEntry {
                seq: 0, // 系統獨立提示
                channel: StreamChannel::System,
                line: message,
                timestamp: Utc::now(),
            }],
        });
    }

    fn emit_error(&self, code: &str, message: &str) {
        let _ = self.event_tx.send(ServerEvent::Error {
            task_id: Some(self.task_id),
            code: code.to_string(),
            message: message.to_string(),
        });
    }
}

```

---

## 2. Semaphore 並行排程與中止控制器：`backend-rust/src/executor/scheduler.rs`

`scheduler.rs` 利用 `tokio::sync::Semaphore` 控管同時執行的 CLI 數量，並透過 `DashMap` 儲存每個任務的 `oneshot::Sender`，讓使用者無論在「排隊中（Queued）」還是「執行中（Running）」都能即時取消任務。

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::{broadcast, oneshot, Semaphore};
use uuid::Uuid;

use crate::executor::runner::TaskRunner;
use crate::models::protocol::{CreateTaskRequest, ServerEvent, TaskMetadata, TaskState};

pub struct TaskScheduler {
    /// 控制最大同時執行的 CLI 數量
    semaphore: Arc<Semaphore>,
    max_concurrency: usize,
    /// 追蹤目前等待中與執行中的任務計數
    queued_count: Arc<AtomicUsize>,
    running_count: Arc<AtomicUsize>,
    /// 儲存活躍任務的中止訊號發送端 (Task ID -> Abort Sender)
    abort_handles: Arc<DashMap<Uuid, oneshot::Sender<()>>>,
    /// 全域事件廣播頻道 (供 WebSocket 訂閱)
    event_tx: broadcast::Sender<ServerEvent>,
    /// 允許的基準工作目錄根路徑
    base_workspace: PathBuf,
}

impl TaskScheduler {
    pub fn new(
        max_concurrency: usize,
        base_workspace: PathBuf,
    ) -> (Self, broadcast::Receiver<ServerEvent>) {
        // 建立容量為 4096 的廣播佇列，供應所有前端 WebSocket 連線
        let (event_tx, event_rx) = broadcast::channel(4096);

        let scheduler = Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            max_concurrency,
            queued_count: Arc::new(AtomicUsize::new(0)),
            running_count: Arc::new(AtomicUsize::new(0)),
            abort_handles: Arc::new(DashMap::new()),
            event_tx,
            base_workspace,
        };

        (scheduler, event_rx)
    }

    /// 取得廣播 Receiver（供新連入的 WebSocket 客戶端呼叫）
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    /// 提交新任務進入排程佇列
    pub fn enqueue(&self, req: CreateTaskRequest) -> TaskMetadata {
        let task_id = Uuid::new_v4();
        let now = Utc::now();

        let metadata = TaskMetadata {
            task_id,
            prompt: req.prompt.clone(),
            workspace_path: req.workspace_path.clone(),
            state: TaskState::Queued,
            exit_code: None,
            created_at: now,
            started_at: None,
            finished_at: None,
        };

        // 註冊中止通道
        let (abort_tx, mut abort_rx) = oneshot::channel::<()>();
        self.abort_handles.insert(task_id, abort_tx);

        // 更新佇列計數並廣播初始狀態
        self.queued_count.fetch_add(1, Ordering::SeqCst);
        let _ = self.event_tx.send(ServerEvent::TaskStateChanged {
            task_id,
            old_state: None,
            new_state: TaskState::Queued,
            exit_code: None,
            timestamp: now,
        });
        self.broadcast_metrics();

        // 複製 Arc 參考以移交給背景 Tokio Task
        let sem = self.semaphore.clone();
        let queued = self.queued_count.clone();
        let running = self.running_count.clone();
        let abort_map = self.abort_handles.clone();
        let event_tx = self.event_tx.clone();
        let base_workspace = self.base_workspace.clone();

        tokio::spawn(async move {
            // 階段 1：在 Semaphore 佇列中等待許可，同時監聽是否在排隊期間就被取消
            let permit = tokio::select! {
                res = sem.acquire_owned() => {
                    match res {
                        Ok(p) => p,
                        Err(_) => return, // Semaphore 已關閉
                    }
                }
                _ = &mut abort_rx => {
                    // 在排隊期間即收到取消指令，直接標記為 Cancelled 並退出
                    queued.fetch_sub(1, Ordering::SeqCst);
                    abort_map.remove(&task_id);
                    let _ = event_tx.send(ServerEvent::TaskStateChanged {
                        task_id,
                        old_state: Some(TaskState::Queued),
                        new_state: TaskState::Cancelled,
                        exit_code: None,
                        timestamp: Utc::now(),
                    });
                    return;
                }
            };

            // 階段 2：成功取得 Semaphore 許可，由 Queued 轉為 Running
            queued.fetch_sub(1, Ordering::SeqCst);
            running.fetch_add(1, Ordering::SeqCst);

            let _ = event_tx.send(ServerEvent::TaskStateChanged {
                task_id,
                old_state: Some(TaskState::Queued),
                new_state: TaskState::Running,
                exit_code: None,
                timestamp: Utc::now(),
            });
            Self::send_metrics(&event_tx, &running, &queued, sem.available_permits());

            // 階段 3：交由 TaskRunner 啟動 CLI 子行程並監控 I/O
            let runner = TaskRunner {
                task_id,
                req,
                base_workspace,
                event_tx: event_tx.clone(),
            };

            let _final_state = runner.run(abort_rx).await;

            // 階段 4：清理資源並釋放 Semaphore 許可給下一位排隊者
            drop(permit);
            running.fetch_sub(1, Ordering::SeqCst);
            abort_map.remove(&task_id);
            Self::send_metrics(&event_tx, &running, &queued, sem.available_permits());
        });

        metadata
    }

    /// 中止指定任務（無論該任務處於 Queued 或 Running 狀態皆可攔截）
    pub fn abort_task(&self, task_id: Uuid) -> bool {
        if let Some((_, abort_tx)) = self.abort_handles.remove(&task_id) {
            let _ = abort_tx.send(());
            self.broadcast_metrics();
            true
        } else {
            false
        }
    }

    /// 廣播當前佇列與並行負載指標
    pub fn broadcast_metrics(&self) {
        let _ = self.event_tx.send(ServerEvent::SystemMetrics {
            running_tasks: self.running_count.load(Ordering::Relaxed),
            max_concurrency: self.max_concurrency,
            queued_tasks: self.queued_count.load(Ordering::Relaxed),
        });
    }

    fn send_metrics(
        event_tx: &broadcast::Sender<ServerEvent>,
        running: &AtomicUsize,
        queued: &AtomicUsize,
        _avail: usize,
    ) {
        let r = running.load(Ordering::Relaxed);
        let q = queued.load(Ordering::Relaxed);
        // 由實際執行的數值推算回傳
        let _ = event_tx.send(ServerEvent::SystemMetrics {
            running_tasks: r,
            max_concurrency: r + _avail,
            queued_tasks: q,
        });
    }
}

```

---

## 3. 架構運作的關鍵防禦機制說明

- **排隊期與執行期雙階段中止（Two-Phase Cancellation）**：
  在 `scheduler.rs` 中，`abort_rx` 先在 `sem.acquire_owned()` 階段透過 `tokio::select!` 參與等待；若任務還在排隊就被取消，不會消耗任何 CLI 啟動成本。一旦取得許可進入執行階段，同一個 `abort_rx` 被移交（Move）給 `TaskRunner::run`，轉而負責觸發 `child.kill().await`。
- **雙重防禦的 50ms 視窗緩衝（Time + Size Windowing）**：
  在 `runner.rs` 的 `run_log_batcher` 中，除了每 50ms 觸發一次 `ticker.tick()` 將累積的日誌打包發送外，還加上了 `buffer.len() >= 250` 的容量上限。這確保了即使 CLI 在瞬間傾倒數萬行編譯錯誤訊息，後端記憶體也不會無限制膨脹，同時將 WebSocket 封包頻率穩定壓制在每秒 20 次（20 FPS）左右，前端 Xterm.js 渲染極度絲滑。
- **管道 EOF 保證（Zero-Lost-Log Guarantee）**：
  在 `child.wait()` 結束後，程式明確呼叫了 `tokio::join!(stdout_task, stderr_task)` 與 `batcher_handle.await`。這保證了即使子行程瞬間秒退（例如參數錯誤立即崩潰），最後幾毫秒印在 `stderr` 的錯誤訊息也一定會被完整打包推送到前端後，才會發送 `TaskStateChanged(Failed)` 事件，不會發生「狀態已顯示失敗、但看不到最後一行報錯原因」的 Race Condition。

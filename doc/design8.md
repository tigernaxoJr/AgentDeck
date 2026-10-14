為了確保當你執行 `systemctl --user stop` 或按下 `Ctrl+C` 時，背景不會殘留任何正在消耗 Token 的孤兒 CLI 行程（Orphan Processes），同時讓 SQLite 有足夠時間將所有中斷任務的狀態更新為 `CANCELLED`，我們需要實作「三階段優雅關閉（Three-Phase Graceful Shutdown）」：

1. **封鎖新進任務（Close Semaphore）**：關閉 `Semaphore`，讓所有還在排隊的任務立即退出。
2. **觸發群體中止與排空等待（Broadcast Abort & Drain）**：走訪 `abort_handles` 發送中止訊號，透過 `TaskRunner` 的 `child.kill().await` 殺死子行程，並利用 **`tokio::util::task::TaskTracker`（或 `AtomicUsize` + `Notify`）** 等待所有子行程的 `stdout`/`stderr` 緩衝與 SQLite 狀態寫入完成。
3. **平滑關閉資料庫連線池（Close SQLite Pool）**：確保 WAL 日誌完整 Checkpoint 後才退出主程式。

以下為 `TaskScheduler` 的擴充實作與整合了 SQLite 的完整版 `main.rs`。

---

## 1. 擴充 `TaskScheduler`：加入 `shutdown_and_drain` 機制

在 `backend-rust/src/executor/scheduler.rs` 中，我們引入 `tokio::sync::Notify`，讓排程器能夠精確得知「最後一個執行中的 CLI 子行程何時完全清理完畢」：

```rust
// 於 scheduler.rs 頂部新增引用
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;

pub struct TaskScheduler {
    semaphore: Arc<Semaphore>,
    max_concurrency: usize,
    queued_count: Arc<AtomicUsize>,
    running_count: Arc<AtomicUsize>,
    abort_handles: Arc<DashMap<Uuid, oneshot::Sender<()>>>,
    event_tx: broadcast::Sender<ServerEvent>,
    base_workspace: PathBuf,
    /// 用於 Graceful Shutdown：當所有活躍任務皆退場時發出通知
    drain_notify: Arc<Notify>,
}

```

### 修改 `TaskScheduler::new` 與 `enqueue` 尾端的資源釋放邏輯：

```rust
impl TaskScheduler {
    pub fn new(
        max_concurrency: usize,
        base_workspace: PathBuf,
    ) -> (Self, broadcast::Receiver<ServerEvent>) {
        let (event_tx, event_rx) = broadcast::channel(4096);

        let scheduler = Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            max_concurrency,
            queued_count: Arc::new(AtomicUsize::new(0)),
            running_count: Arc::new(AtomicUsize::new(0)),
            abort_handles: Arc::new(DashMap::new()),
            event_tx,
            base_workspace,
            drain_notify: Arc::new(Notify::new()),
        };

        (scheduler, event_rx)
    }

    pub fn enqueue(&self, req: CreateTaskRequest) -> TaskMetadata {
        // ... (前段建立 metadata 與 abort_handles 邏輯不變) ...

        let drain_notify = self.drain_notify.clone();
        // ... (其餘 Arc clone) ...

        tokio::spawn(async move {
            // 階段 1：等待 Semaphore 許可
            let permit = tokio::select! {
                res = sem.acquire_owned() => {
                    match res {
                        Ok(p) => p,
                        Err(_) => {
                            // 當執行 Graceful Shutdown 呼叫 sem.close() 時會進到此分支
                            queued.fetch_sub(1, Ordering::SeqCst);
                            abort_map.remove(&task_id);
                            let _ = event_tx.send(ServerEvent::TaskStateChanged {
                                task_id,
                                old_state: Some(TaskState::Queued),
                                new_state: TaskState::Cancelled,
                                exit_code: None,
                                timestamp: Utc::now(),
                            });
                            if running.load(Ordering::SeqCst) == 0 && queued.load(Ordering::SeqCst) == 0 {
                                drain_notify.notify_waiters();
                            }
                            return;
                        }
                    }
                }
                _ = &mut abort_rx => {
                    queued.fetch_sub(1, Ordering::SeqCst);
                    abort_map.remove(&task_id);
                    let _ = event_tx.send(ServerEvent::TaskStateChanged {
                        task_id,
                        old_state: Some(TaskState::Queued),
                        new_state: TaskState::Cancelled,
                        exit_code: None,
                        timestamp: Utc::now(),
                    });
                    if running.load(Ordering::SeqCst) == 0 && queued.load(Ordering::SeqCst) == 0 {
                        drain_notify.notify_waiters();
                    }
                    return;
                }
            };

            // ... (階段 2 & 3：切換為 Running 並執行 runner.run(abort_rx).await 不變) ...

            // 階段 4：清理資源並檢查是否所有任務皆已排空
            drop(permit);
            let remaining_running = running.fetch_sub(1, Ordering::SeqCst) - 1;
            abort_map.remove(&task_id);
            Self::send_metrics(&event_tx, &running, &queued, sem.available_permits());

            if remaining_running == 0 && queued.load(Ordering::SeqCst) == 0 {
                drain_notify.notify_waiters();
            }
        });

        metadata
    }

    /// 【新增】優雅關閉：封鎖佇列、向所有活躍 CLI 發送中止訊號，並等待子行程完全退出
    pub async fn shutdown_and_drain(&self, grace_period: Duration) {
        println!("🛑 [Scheduler] 開始執行優雅關閉，正在清理排隊與執行中的 CLI 任務...");

        // 1. 關閉 Semaphore，拒絕新任務並讓所有阻塞在 acquire_owned() 的排隊任務立即釋放
        self.semaphore.close();

        // 2. 收集目前所有活躍的 Task ID 並觸發 Abort 訊號
        let active_ids: Vec<Uuid> = self.abort_handles.iter().map(|entry| *entry.key()).collect();
        let active_count = active_ids.len();

        if active_count > 0 {
            println!("⚡ [Scheduler] 正在向 {} 個活躍任務發送終止訊號...", active_count);
            for id in active_ids {
                if let Some((_, tx)) = self.abort_handles.remove(&id) {
                    let _ = tx.send(());
                }
            }
        }

        // 3. 等待所有 Running 與 Queued 計數歸零（搭配超時保護）
        let wait_drain = async {
            loop {
                let r = self.running_count.load(Ordering::SeqCst);
                let q = self.queued_count.load(Ordering::SeqCst);
                if r == 0 && q == 0 {
                    break;
                }
                self.drain_notify.notified().await;
            }
        };

        match timeout(grace_period, wait_drain).await {
            Ok(_) => println!("✅ [Scheduler] 所有 Antigravity CLI 子行程已安全終止並清理完畢。"),
            Err(_) => eprintln!(
                "⚠️ [Scheduler] 等待子行程退出超時 ({:?})，將由 OS 強制回收殘留資源。",
                grace_period
            ),
        }
    }
}

```

---

## 2. 補充 `TaskRepository::close` 方法

在 `backend-rust/src/storage/db.rs` 的 `impl TaskRepository` 區塊尾端加入關閉連線池的輔助函式，確保廣播事件最後一筆 `CANCELLED` 寫入 SQLite 後才關閉檔案鎖：

```rust
    /// 優雅關閉 SQLite 連線池，確保 WAL 完整寫入磁碟
    pub async fn close(&self) {
        // 給予背景 Event Sink 150ms 的緩衝時間消化最後一波 TaskStateChanged(Cancelled) 事件
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.pool.close().await;
        println!("💾 [SQLite] 資料庫連線池已安全關閉。");
    }

```

---

## 3. 完整版 `backend-rust/src/main.rs`（整合 SIGTERM 攔截與三階段關閉）

在 `main.rs` 中，我們使用 `tokio::signal` 同時監聽 **`SIGINT` (`Ctrl+C`)** 與 **`SIGTERM` (`systemctl --user stop`)**，並串聯 Axum 的 `with_graceful_shutdown`：

```rust
mod api;
mod executor;
mod models;
mod storage;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use api::routes::{create_router, AppState};
use executor::scheduler::TaskScheduler;
use storage::db::TaskRepository;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // 1. 讀取環境變數設定
    let base_workspace = std::env::var("ANTIGRAVITY_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().expect("無法取得當前目錄"));

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://antigravity_tasks.db".to_string());

    // 2. 初始化 SQLite 儲存層 (含 WAL 模式與孤兒任務修復)
    let repo = TaskRepository::init(&db_url)
        .await
        .expect("無法初始化 SQLite 資料庫");

    // 3. 初始化並行排程器並掛載 SQLite 背景事件同步器
    let max_concurrency = 2;
    let (scheduler, event_rx) = TaskScheduler::new(max_concurrency, base_workspace.clone());
    repo.spawn_event_sink(event_rx);

    // 4. 建立共享狀態與 Axum Router
    let app_state = AppState::new(scheduler, repo.clone());
    let scheduler_ref = app_state.scheduler.clone();
    let app = create_router(app_state);

    // 5. 綁定連接埠並掛載 Graceful Shutdown 訊號監聽
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("🚀 Antigravity Web-CLI Bridge 啟動於 http://{}", addr);
    println!("📂 允許的基準工作目錄: {:?}", base_workspace);
    println!("🗄️ 資料庫路徑: {}", db_url);

    let listener = TcpListener::bind(addr).await.expect("無法綁定連接埠 3000");

    // 啟動 Axum Server，當收到 SIGINT 或 SIGTERM 時停止接受新的 HTTP/WS 連線
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Axum 伺服器執行異常");

    // 6. 當 HTTP Server 停止收單後，給予 8 秒時間終止所有 CLI 子行程並排空佇列
    // (配合 Systemd 設定檔中的 TimeoutStopSec=10，預留 2 秒緩衝)
    scheduler_ref
        .shutdown_and_drain(Duration::from_secs(8))
        .await;

    // 7. 將最後的任務取消狀態寫入 SQLite 並關閉連線池
    repo.close().await;
    println!("👋 服務已完全停止。");
}

/// 跨平台監聽系統終止訊號 (Ctrl+C / SIGINT 與 Systemd SIGTERM)
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("無法安裝 Ctrl+C 訊號處理器");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("無法安裝 SIGTERM 訊號處理器")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("\n⚠️ 收到 SIGINT (Ctrl+C) 訊號，準備關閉服務...");
        },
        _ = terminate => {
            println!("\n⚠️ 收到 SIGTERM (Systemd Stop) 訊號，準備關閉服務...");
        },
    }
}

```

---

## 4. 優雅關閉的時間軸對齊設計 (Timing Alignment)

在先前的 `antigravity-web.service` Systemd 設定檔中，我們配置了 `TimeoutStopSec=10`。這與 `main.rs` 形成了嚴密的退場時間軸：

- **`T = 0.0s`**：管理員執行 `systemctl --user stop antigravity-web`，Systemd 發送 `SIGTERM` 給 Rust 主行程。
- **`T = 0.01s`**：`shutdown_signal()` 被觸發，Axum 停止接受新的 HTTP 請求，並啟動 `scheduler_ref.shutdown_and_drain(8s)`。
- **`T = 0.05s`**：所有執行中的 `TaskRunner` 從 `abort_rx` 收到訊號，立即呼叫 `child.kill().await` 終止底層的 `antigravity` CLI 子行程，並向廣播頻道發送最後一筆系統 Log 與 `TaskStateChanged(Cancelled)`。
- **`T = 0.10s ~ 0.25s`**：`repo.close().await` 等待 150ms 讓背景 Event Sink 將所有 `CANCELLED` 狀態寫入 SQLite，接著關閉資料庫連線池並執行 WAL Checkpoint。
- **`T < 0.5s`**：Rust 主行程以 `ExitCode = 0` 乾淨退出，遠低於 Systemd 的 10 秒強制 `SIGKILL` 死線。

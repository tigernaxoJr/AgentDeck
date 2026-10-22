use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::{broadcast, oneshot, Notify, Semaphore};
use tokio::time::timeout;
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
    /// 用於 Graceful Shutdown：當所有活躍任務皆退場時發出通知
    drain_notify: Arc<Notify>,
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
            drain_notify: Arc::new(Notify::new()),
        };

        (scheduler, event_rx)
    }

    pub fn base_workspace(&self) -> &PathBuf {
        &self.base_workspace
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
            model: req.model.clone(),
            state: TaskState::Queued,
            exit_code: None,
            created_at: now,
            started_at: None,
            finished_at: None,
            usage: None,
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
            usage: None,
        });
        self.broadcast_metrics();

        // 複製 Arc 參考以移交給背景 Tokio Task
        let sem = self.semaphore.clone();
        let queued = self.queued_count.clone();
        let running = self.running_count.clone();
        let abort_map = self.abort_handles.clone();
        let event_tx = self.event_tx.clone();
        let base_workspace = self.base_workspace.clone();
        let drain_notify = self.drain_notify.clone();

        tokio::spawn(async move {
            // 階段 1：在 Semaphore 佇列中等待許可，同時監聽是否在排隊期間就被取消
            let permit = tokio::select! {
                res = sem.clone().acquire_owned() => {
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
                        usage: None,
                    });
                    if running.load(Ordering::Relaxed) == 0 && queued.load(Ordering::Relaxed) == 0 {
                        drain_notify.notify_waiters();
                    }
                    return;
                }
            };

            // 階段 2：成功取得 Semaphore 許可，由 Queued 轉為 Running
            queued.fetch_sub(1, Ordering::SeqCst);
            running.fetch_add(1, Ordering::SeqCst);

            self_send_metrics(&event_tx, &running, &queued, sem.available_permits());
            let _ = event_tx.send(ServerEvent::TaskStateChanged {
                task_id,
                old_state: Some(TaskState::Queued),
                new_state: TaskState::Running,
                exit_code: None,
                timestamp: Utc::now(),
                usage: None,
            });

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
            self_send_metrics(&event_tx, &running, &queued, sem.available_permits());

            if running.load(Ordering::Relaxed) == 0 && queued.load(Ordering::Relaxed) == 0 {
                drain_notify.notify_waiters();
            }
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

    /// 三階段優雅關閉排程器與等待子行程排空
    pub async fn shutdown_and_drain(&self, grace_period: Duration) {
        println!("🛑 [Scheduler] 正在關閉並行 Semaphore，停止接單...");
        self.semaphore.close();

        // 走訪 abort_handles 中止所有活躍任務
        let active_tasks: Vec<Uuid> = self.abort_handles.iter().map(|entry| *entry.key()).collect();
        for task_id in active_tasks {
            self.abort_task(task_id);
        }

        let wait_drain = async {
            while self.running_count.load(Ordering::Relaxed) > 0
                || self.queued_count.load(Ordering::Relaxed) > 0
            {
                self.drain_notify.notified().await;
            }
        };

        match timeout(grace_period, wait_drain).await {
            Ok(_) => println!("✅ [Scheduler] 所有 CLI 子行程已安全終止並清理完畢。"),
            Err(_) => eprintln!(
                "⚠️ [Scheduler] 等待子行程退出超時 ({:?})，將由 OS 強制回收殘留資源。",
                grace_period
            ),
        }
    }
}

fn self_send_metrics(
    event_tx: &broadcast::Sender<ServerEvent>,
    running: &AtomicUsize,
    queued: &AtomicUsize,
    avail: usize,
) {
    let r = running.load(Ordering::Relaxed);
    let q = queued.load(Ordering::Relaxed);
    let _ = event_tx.send(ServerEvent::SystemMetrics {
        running_tasks: r,
        max_concurrency: r + avail,
        queued_tasks: q,
    });
}

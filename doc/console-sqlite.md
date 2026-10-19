這份補充設計將原本暫存於記憶體（`DashMap`）的任務狀態與 Artifact 產出落地至 **SQLite（採用 `sqlx` 並開啟 WAL 高併發模式）**，並提供 **Linux Systemd（特別推薦使用 User Service 模式）** 的常駐設定，確保服務重啟時能自動清理異常中斷的孤兒任務，同時完整繼承你個人帳號的 Antigravity CLI Token 憑證。

---

## 1. SQLite 資料庫 Schema 與 WAL 優化設計

為了因應 WebSocket 廣播期間可能發生的頻繁狀態更新與讀取，SQLite 必須啟用 **`PRAGMA journal_mode = WAL;`（Write-Ahead Logging）**，讓「寫入狀態」與「前端讀取歷史清單」互不阻塞。

### 資料表結構（由程式啟動時自動建立）

我們將「任務元資料 + 最終結構化 Artifact」存放在主表 `tasks`，並可選擇性地將歷史 Log 批次歸檔於 `task_logs`：

```sql
CREATE TABLE IF NOT EXISTS tasks (
    task_id TEXT PRIMARY KEY NOT NULL,
    prompt TEXT NOT NULL,
    workspace_path TEXT NOT NULL,
    state TEXT NOT NULL,               -- QUEUED, RUNNING, COMPLETED, FAILED, CANCELLED, TIMEOUT
    exit_code INTEGER,
    created_at TEXT NOT NULL,          -- RFC3339 / ISO8601 UTC 字串
    started_at TEXT,
    finished_at TEXT,
    artifact_file_name TEXT,           -- 若有產出結構化摘要則記錄檔名
    artifact_type TEXT,                -- json, markdown, diff, plaintext
    artifact_content TEXT              -- JSON 序列化字串
);

CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);

```

---

## 2. SQLx 持久化模組實作：`backend-rust/src/storage/db.rs`

在 `Cargo.toml` 中加入：

```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono", "uuid"] }

```

以下實作包含四大關鍵功能：

1. **自動建立資料庫與開啟 WAL 模式**。
2. **孤兒任務修復（Orphan Recovery）**：若後端服務因斷電或重啟而中斷，啟動時會自動將上次殘留為 `RUNNING` 或 `QUEUED` 的任務標記為 `FAILED (ExitCode: -1)`，防止前端看到永遠卡在執行中的幽靈任務。
3. **背景事件自動落盤（Background Event Sink）**：直接訂閱 `TaskScheduler` 的廣播頻道，當任務狀態改變或產出 Artifact 時自動非同步寫入 SQLite，完全不侵入 `runner.rs` 的核心邏輯。

```rust
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, SqlitePool,
};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::models::protocol::{
    ArtifactData, ArtifactType, ServerEvent, TaskMetadata, TaskState,
};

#[derive(Clone)]
pub struct TaskRepository {
    pool: SqlitePool,
}

impl TaskRepository {
    /// 初始化 SQLite 連線池、開啟 WAL 模式並執行自動 Migration 與孤兒任務清理
    pub async fn init(db_url: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(db_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let repo = Self { pool };
        repo.run_migrations().await?;
        repo.recover_orphan_tasks().await?;

        Ok(repo)
    }

    async fn run_migrations(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                task_id TEXT PRIMARY KEY NOT NULL,
                prompt TEXT NOT NULL,
                workspace_path TEXT NOT NULL,
                state TEXT NOT NULL,
                exit_code INTEGER,
                created_at TEXT NOT NULL,
                started_at TEXT,
                finished_at TEXT,
                artifact_file_name TEXT,
                artifact_type TEXT,
                artifact_content TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at DESC);
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 系統重啟時，將上次異常中斷前仍處於 QUEUED / RUNNING 的任務標記為 FAILED
    async fn recover_orphan_tasks(&self) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE tasks
            SET state = 'FAILED',
                exit_code = -1,
                finished_at = ?1
            WHERE state IN ('QUEUED', 'RUNNING')
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 新增任務紀錄
    pub async fn insert_task(&self, task: &TaskMetadata) -> Result<(), sqlx::Error> {
        let state_str = serde_json::to_value(task.state)
            .unwrap()
            .as_str()
            .unwrap_or("QUEUED")
            .to_string();

        sqlx::query(
            r#"
            INSERT INTO tasks (task_id, prompt, workspace_path, state, exit_code, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(task.task_id.to_string())
        .bind(&task.prompt)
        .bind(&task.workspace_path)
        .bind(state_str)
        .bind(task.exit_code)
        .bind(task.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 更新任務狀態與時間戳
    pub async fn update_task_state(
        &self,
        task_id: Uuid,
        new_state: TaskState,
        exit_code: Option<i32>,
        timestamp: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let state_str = serde_json::to_value(new_state)
            .unwrap()
            .as_str()
            .unwrap_or("FAILED")
            .to_string();
        let ts_str = timestamp.to_rfc3339();

        match new_state {
            TaskState::Running => {
                sqlx::query("UPDATE tasks SET state = ?1, started_at = ?2 WHERE task_id = ?3")
                    .bind(state_str)
                    .bind(ts_str)
                    .bind(task_id.to_string())
                    .execute(&self.pool)
                    .await?;
            }
            TaskState::Completed
            | TaskState::Failed
            | TaskState::Cancelled
            | TaskState::Timeout => {
                sqlx::query(
                    "UPDATE tasks SET state = ?1, exit_code = ?2, finished_at = ?3 WHERE task_id = ?4",
                )
                .bind(state_str)
                .bind(exit_code)
                .bind(ts_str)
                .bind(task_id.to_string())
                .execute(&self.pool)
                .await?;
            }
            TaskState::Queued => {}
        }
        Ok(())
    }

    /// 儲存結構化產出 (Artifact)
    pub async fn save_artifact(
        &self,
        task_id: Uuid,
        artifact: &ArtifactData,
    ) -> Result<(), sqlx::Error> {
        let type_str = serde_json::to_value(artifact.artifact_type)
            .unwrap()
            .as_str()
            .unwrap_or("plaintext")
            .to_string();
        let content_str = serde_json::to_string(&artifact.content).unwrap_or_default();

        sqlx::query(
            r#"
            UPDATE tasks
            SET artifact_file_name = ?1,
                artifact_type = ?2,
                artifact_content = ?3
            WHERE task_id = ?4
            "#,
        )
        .bind(&artifact.file_name)
        .bind(type_str)
        .bind(content_str)
        .bind(task_id.to_string())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 查詢最近 N 筆任務清單（供前端初始化載入）
    pub async fn list_recent_tasks(&self, limit: i64) -> Result<Vec<TaskMetadata>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT task_id, prompt, workspace_path, state, exit_code, created_at, started_at, finished_at
            FROM tasks
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut list = Vec::with_capacity(rows.len());
        for row in rows {
            let id_str: String = row.get("task_id");
            let state_str: String = row.get("state");
            let created_str: String = row.get("created_at");
            let started_str: Option<String> = row.get("started_at");
            let finished_str: Option<String> = row.get("finished_at");

            list.push(TaskMetadata {
                task_id: Uuid::parse_str(&id_str).unwrap_or_default(),
                prompt: row.get("prompt"),
                workspace_path: row.get("workspace_path"),
                state: serde_json::from_value(Value::String(state_str))
                    .unwrap_or(TaskState::Failed),
                exit_code: row.get("exit_code"),
                created_at: DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                started_at: started_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
                finished_at: finished_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
            });
        }
        Ok(list)
    }

    /// 啟動背景監聽器，自動將廣播頻道中的狀態更新與 Artifact 寫入 SQLite
    pub fn spawn_event_sink(&self, mut rx: broadcast::Receiver<ServerEvent>) {
        let repo = self.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                match event {
                    ServerEvent::TaskStateChanged {
                        task_id,
                        new_state,
                        exit_code,
                        timestamp,
                        ..
                    } => {
                        let _ = repo
                            .update_task_state(task_id, new_state, exit_code, timestamp)
                            .await;
                    }
                    ServerEvent::ArtifactReady { task_id, artifact } => {
                        let _ = repo.save_artifact(task_id, &artifact).await;
                    }
                    _ => {}
                }
            }
        });
    }
}

```

> **與 `AppState` 整合方式**：
> 在 `main.rs` 啟動時呼叫 `let repo = TaskRepository::init("sqlite://antigravity_tasks.db").await?;`，接著呼叫 `repo.spawn_event_sink(scheduler.subscribe());`。而在 `routes.rs` 的 `create_task_handler` 中呼叫 `repo.insert_task(&metadata).await`，並將 `list_tasks_handler` 改為呼叫 `repo.list_recent_tasks(100).await`，即可移除原本的記憶體 `DashMap`。

---

## 3. Linux Systemd 常駐背景服務配置

由於 Antigravity CLI 的 OAuth 憑證、Session Token 與相關環境設定通常儲存在你**個人使用者家目錄**（如 `~/.config/` 或 `~/.antigravity/`）下，若使用傳統 `root` 系統級服務啟動，很容易遇到找不到憑證或建立出來的專案檔案權限變成 `root` 的問題。

因此，最佳實踐是採用 **Systemd User Service（使用者級常駐服務）**，並開啟 `loginctl enable-linger`，讓服務在開機時（即使你尚未登入桌面或 SSH）也能以你的個人身分在背景自動啟動。

### 步驟 1：建立單元設定檔 `~/.config/systemd/user/antigravity-web.service`

```ini
[Unit]
Description=Antigravity CLI Web Console Bridge (Rust Axum)
After=network.target

[Service]
Type=simple
# 執行編譯好的 Release Binary
ExecStart=%h/.local/bin/antigravity-web-bridge

# 預設工作目錄設為你的集中開發資料夾
WorkingDirectory=%h/workspaces

# 注入關鍵環境變數：確保能找到 antigravity CLI 執行檔與 SQLite 路徑
Environment="PATH=%h/.local/bin:%h/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
Environment="ANTIGRAVITY_WORKSPACE=%h/workspaces"
Environment="DATABASE_URL=sqlite://%h/.local/share/antigravity-web/tasks.db"
Environment="RUST_LOG=info,antigravity_web=debug"

# 崩潰自動重啟策略
Restart=on-failure
RestartSec=3

# 優雅終止設定：當執行 systemctl stop 時，給予 Rust 10 秒時間清理執行中的 CLI 子行程
TimeoutStopSec=10
KillMode=mixed
KillSignal=SIGTERM

# 基本資源與安全防護
NoNewPrivileges=true
LimitNOFILE=65535

[Install]
WantedBy=default.target

```

_(註：Systemd User Service 中的 `%h` 會自動展開為你的家目錄 `/home/你的帳號`)_

### 步驟 2：部署與啟用開機自啟指令

將編譯好的二進位檔與資料庫目錄準備好後，依序執行以下指令即可完成常駐化：

```bash
# 1. 建立二進位檔與 SQLite 存放目錄
mkdir -p ~/.local/bin ~/.local/share/antigravity-web ~/workspaces

# 2. 編譯 Rust 後端並複製過去
cargo build --release
cp target/release/antigravity-web-bridge ~/.local/bin/

# 3. 重新載入使用者級 Systemd 並啟動服務
systemctl --user daemon-reload
systemctl --user enable --now antigravity-web.service

# 4. 【關鍵】開啟 Linger 模式：確保伺服器重開機、尚未 SSH 登入時，服務依然自動在背景執行
sudo loginctl enable-linger $USER

# 5. 查看即時運行狀態與日誌
systemctl --user status antigravity-web.service
journalctl --user -u antigravity-web.service -f

```

### 步驟 3：（選配）讓 Rust 單一執行檔同時託管 Vue 3 靜態網頁

為了讓 Systemd 只需要管理一個執行檔，你可以在前端執行 `npm run build` 產生 `dist/` 資料夾後，於 Rust Axum 的 `create_router` 尾端加上 `tower_http::services::ServeDir`：

```rust
use tower_http::services::ServeDir;

// 加在 create_router() 的最後：若非 /api 開頭的請求，直接回傳 Vue 3 編譯好的靜態檔案
.fallback_service(ServeDir::new("./frontend-dist"))

```

如此一來，整個系統只需要啟動這一個 Rust Systemd 服務，打開瀏覽器連上 `[http://127.0.0.1:3000](http://127.0.0.1:3000)` 就能直接使用完整的 Vue 3 控制台。

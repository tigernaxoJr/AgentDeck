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
    ArtifactData, ServerEvent, TaskMetadata, TaskState,
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
            CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);
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

    /// 查詢單一任務
    pub async fn get_task_by_id(&self, task_id: Uuid) -> Result<Option<TaskMetadata>, sqlx::Error> {
        let row_opt = sqlx::query(
            r#"
            SELECT task_id, prompt, workspace_path, state, exit_code, created_at, started_at, finished_at
            FROM tasks
            WHERE task_id = ?1
            "#,
        )
        .bind(task_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row_opt {
            let id_str: String = row.get("task_id");
            let state_str: String = row.get("state");
            let created_str: String = row.get("created_at");
            let started_str: Option<String> = row.get("started_at");
            let finished_str: Option<String> = row.get("finished_at");

            Ok(Some(TaskMetadata {
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
            }))
        } else {
            Ok(None)
        }
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

    /// 優雅關閉 SQLite 連線池，確保 WAL 完整寫入磁碟
    pub async fn close(&self) {
        // 給予背景 Event Sink 150ms 的緩衝時間消化最後一波 TaskStateChanged 事件
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.pool.close().await;
        println!("💾 [SQLite] 資料庫連線池已安全關閉。");
    }
}

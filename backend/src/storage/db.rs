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

use super::migrate::{self, Migration};
use crate::models::protocol::{
    ArtifactData, ServerEvent, TaskMetadata, TaskState,
};

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    sql: r#"
        CREATE TABLE IF NOT EXISTS tasks (
            task_id TEXT PRIMARY KEY NOT NULL,
            prompt TEXT NOT NULL,
            workspace_path TEXT NOT NULL,
            model TEXT,
            state TEXT NOT NULL,
            exit_code INTEGER,
            created_at TEXT NOT NULL,
            started_at TEXT,
            finished_at TEXT,
            artifact_file_name TEXT,
            artifact_type TEXT,
            artifact_content TEXT,
            input_tokens INTEGER,
            output_tokens INTEGER,
            thinking_tokens INTEGER,
            cache_read_tokens INTEGER,
            total_tokens INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);
    "#,
}];

/// 版本化遷移之前陸續以 ALTER TABLE 加入的欄位
const LEGACY_ADDED_COLUMNS: &[(&str, &str)] = &[
    ("model", "TEXT"),
    ("input_tokens", "INTEGER"),
    ("output_tokens", "INTEGER"),
    ("thinking_tokens", "INTEGER"),
    ("cache_read_tokens", "INTEGER"),
    ("total_tokens", "INTEGER"),
];

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
        if migrate::current_version(&self.pool).await? == 0 {
            self.upgrade_legacy_tasks_table().await?;
        }
        migrate::run(&self.pool, MIGRATIONS).await
    }

    /// 版本化遷移之前建立的舊資料庫可能缺少後來新增的欄位，先補齊再交給 v1 基準
    async fn upgrade_legacy_tasks_table(&self) -> Result<(), sqlx::Error> {
        let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('tasks')")
            .fetch_all(&self.pool)
            .await?;
        if columns.is_empty() {
            return Ok(());
        }
        for (name, ty) in LEGACY_ADDED_COLUMNS {
            if !columns.iter().any(|c| c == name) {
                sqlx::raw_sql(&format!("ALTER TABLE tasks ADD COLUMN {name} {ty}"))
                    .execute(&self.pool)
                    .await?;
            }
        }
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
            INSERT INTO tasks (task_id, prompt, workspace_path, model, state, exit_code, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(task.task_id.to_string())
        .bind(&task.prompt)
        .bind(&task.workspace_path)
        .bind(&task.model)
        .bind(state_str)
        .bind(task.exit_code)
        .bind(task.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 更新任務狀態與時間戳及 Token 使用量
    pub async fn update_task_state(
        &self,
        task_id: Uuid,
        new_state: TaskState,
        exit_code: Option<i32>,
        timestamp: DateTime<Utc>,
        usage: Option<&crate::models::protocol::TokenUsage>,
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
                if let Some(u) = usage {
                    sqlx::query(
                        r#"
                        UPDATE tasks
                        SET state = ?1, exit_code = ?2, finished_at = ?3,
                            input_tokens = ?4, output_tokens = ?5, thinking_tokens = ?6,
                            cache_read_tokens = ?7, total_tokens = ?8
                        WHERE task_id = ?9
                        "#,
                    )
                    .bind(state_str)
                    .bind(exit_code)
                    .bind(ts_str)
                    .bind(u.input_tokens)
                    .bind(u.output_tokens)
                    .bind(u.thinking_tokens)
                    .bind(u.cache_read_tokens)
                    .bind(u.total_tokens)
                    .bind(task_id.to_string())
                    .execute(&self.pool)
                    .await?;
                } else {
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
            SELECT task_id, prompt, workspace_path, model, state, exit_code, created_at, started_at, finished_at,
                   input_tokens, output_tokens, thinking_tokens, cache_read_tokens, total_tokens
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
            let model: Option<String> = row.get("model");
            let total_tokens: Option<i64> = row.get("total_tokens");

            let usage = total_tokens.map(|tot| crate::models::protocol::TokenUsage {
                input_tokens: row.get::<Option<i64>, _>("input_tokens").unwrap_or(0),
                output_tokens: row.get::<Option<i64>, _>("output_tokens").unwrap_or(0),
                thinking_tokens: row.get::<Option<i64>, _>("thinking_tokens").unwrap_or(0),
                cache_read_tokens: row.get::<Option<i64>, _>("cache_read_tokens").unwrap_or(0),
                total_tokens: tot,
            });

            Ok(Some(TaskMetadata {
                task_id: Uuid::parse_str(&id_str).unwrap_or_default(),
                prompt: row.get("prompt"),
                workspace_path: row.get("workspace_path"),
                model,
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
                usage,
            }))
        } else {
            Ok(None)
        }
    }

    /// 查詢最近 N 筆任務清單（供前端初始化載入）
    pub async fn list_recent_tasks(&self, limit: i64) -> Result<Vec<TaskMetadata>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT task_id, prompt, workspace_path, model, state, exit_code, created_at, started_at, finished_at,
                   input_tokens, output_tokens, thinking_tokens, cache_read_tokens, total_tokens
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
            let model: Option<String> = row.get("model");
            let total_tokens: Option<i64> = row.get("total_tokens");

            let usage = total_tokens.map(|tot| crate::models::protocol::TokenUsage {
                input_tokens: row.get::<Option<i64>, _>("input_tokens").unwrap_or(0),
                output_tokens: row.get::<Option<i64>, _>("output_tokens").unwrap_or(0),
                thinking_tokens: row.get::<Option<i64>, _>("thinking_tokens").unwrap_or(0),
                cache_read_tokens: row.get::<Option<i64>, _>("cache_read_tokens").unwrap_or(0),
                total_tokens: tot,
            });

            list.push(TaskMetadata {
                task_id: Uuid::parse_str(&id_str).unwrap_or_default(),
                prompt: row.get("prompt"),
                workspace_path: row.get("workspace_path"),
                model,
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
                usage,
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
                        usage,
                        ..
                    } => {
                        let _ = repo
                            .update_task_state(task_id, new_state, exit_code, timestamp, usage.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_upgrades_legacy_tasks_table() {
        let url = "sqlite:file:legacy_tasks_test?mode=memory&cache=shared";
        // 保持一條連線存活，讓共享記憶體資料庫在 init 前不被釋放
        let legacy = SqlitePool::connect(url).await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE tasks (task_id TEXT PRIMARY KEY NOT NULL, prompt TEXT NOT NULL,
             workspace_path TEXT NOT NULL, state TEXT NOT NULL, exit_code INTEGER,
             created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT,
             artifact_file_name TEXT, artifact_type TEXT, artifact_content TEXT)",
        )
        .execute(&legacy)
        .await
        .unwrap();

        let repo = TaskRepository::init(url).await.unwrap();
        let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('tasks')")
            .fetch_all(&repo.pool)
            .await
            .unwrap();
        for (name, _) in LEGACY_ADDED_COLUMNS {
            assert!(columns.iter().any(|c| c == name), "missing column {name}");
        }
        assert_eq!(migrate::current_version(&repo.pool).await.unwrap(), 1);

        // 再次初始化不應重複套用
        TaskRepository::init(url).await.unwrap();
    }
}

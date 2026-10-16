use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};

use crate::storage::migrate::{self, Migration};

/// tutor.db 的 schema 遷移；新增變更時只能往後追加，不可修改已發佈的檔案
pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "init", sql: include_str!("migrations/0001_init.sql") },
    Migration {
        version: 2,
        name: "constraints",
        sql: include_str!("migrations/0002_constraints.sql"),
    },
];

pub fn latest_version() -> i64 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

pub async fn open(data_dir: &Path) -> Result<SqlitePool, sqlx::Error> {
    tokio::fs::create_dir_all(data_dir).await.map_err(|e| {
        sqlx::Error::Configuration(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to create data dir: {e}"),
        )))
    })?;

    let db_path = data_dir.join("tutor.db");
    let existed = db_path.exists();
    let options = SqliteConnectOptions::new().filename(&db_path).create_if_missing(true);
    let pool = connect(options).await?;

    // 既有資料庫升級前先留一份快照，遷移失敗或資料清理不如預期時可還原
    let current = migrate::current_version(&pool).await?;
    if existed && current < latest_version() {
        let backup = data_dir.join(format!("tutor.db.pre-v{}.bak", latest_version()));
        if !backup.exists() {
            let backup_str = backup.to_string_lossy().replace('\'', "''");
            sqlx::raw_sql(&format!("VACUUM INTO '{backup_str}'")).execute(&pool).await?;
        }
    }

    migrate::run(&pool, MIGRATIONS).await?;
    Ok(pool)
}

pub async fn open_url(conn_str: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = connect(SqliteConnectOptions::from_str(conn_str)?).await?;
    migrate::run(&pool, MIGRATIONS).await?;
    Ok(pool)
}

async fn connect(options: SqliteConnectOptions) -> Result<SqlitePool, sqlx::Error> {
    let options = options
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_schema_creates_tables() {
        let pool = open_url("sqlite::memory:").await.expect("open memory db");
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        )
        .fetch_all(&pool)
        .await
        .expect("fetch tables");

        let names: Vec<String> = tables.into_iter().map(|(n,)| n).collect();
        let expected = vec![
            "ai_calls",
            "attempts",
            "concept_weakness",
            "dialog_turns",
            "mastery",
            "question_concepts",
            "questions",
            "sessions",
            "settings",
            "students",
            "subjects",
            "units",
        ];
        assert_eq!(names, expected);
        assert_eq!(migrate::current_version(&pool).await.unwrap(), latest_version());
    }

    #[tokio::test]
    async fn test_question_concepts_follow_concept_tags() {
        let pool = open_url("sqlite::memory:").await.expect("open memory db");
        sqlx::raw_sql(
            r#"
            INSERT INTO subjects (id, name) VALUES ('social', '社會');
            INSERT INTO units (id, subject_id, title, ord) VALUES ('social_r01', 'social', 'r1', 1);
            INSERT INTO questions (id, subject_id, unit_id, ord, qtype, stem, answer_json,
                                   concept_tags_json, input_mode, updated_at)
            VALUES ('q1', 'social', 'social_r01', 1, 'true_false', 's', 'true',
                    '["地圖-圖例", "方位"]', 'choice', 'now');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let tags = |pool: SqlitePool| async move {
            sqlx::query_scalar::<_, String>(
                "SELECT concept_tag FROM question_concepts WHERE question_id = 'q1' ORDER BY concept_tag",
            )
            .fetch_all(&pool)
            .await
            .unwrap()
        };
        assert_eq!(tags(pool.clone()).await, vec!["地圖-圖例", "方位"]);

        sqlx::query("UPDATE questions SET concept_tags_json = '[\"地圖\"]' WHERE id = 'q1'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(tags(pool.clone()).await, vec!["地圖"]);

        // 題目科目必須與單元科目一致
        let mismatch = sqlx::raw_sql(
            r#"
            INSERT INTO subjects (id, name) VALUES ('nature', '自然');
            INSERT INTO questions (id, subject_id, unit_id, ord, qtype, stem, answer_json,
                                   input_mode, updated_at)
            VALUES ('q2', 'nature', 'social_r01', 1, 'true_false', 's', 'true', 'choice', 'now');
            "#,
        )
        .execute(&pool)
        .await;
        assert!(mismatch.is_err());
    }

    /// 手動驗證：`TUTOR_MIGRATE_DIR=<含 tutor.db 副本的目錄> cargo test -- --ignored migrate_real_db`
    #[tokio::test]
    #[ignore]
    async fn migrate_real_db() {
        let dir = std::env::var("TUTOR_MIGRATE_DIR").expect("TUTOR_MIGRATE_DIR");
        let pool = open(Path::new(&dir)).await.expect("migrate");
        assert_eq!(migrate::current_version(&pool).await.unwrap(), latest_version());
        let violations: Vec<(String,)> = sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(violations.is_empty());
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&pool).await.unwrap();
        assert_eq!(integrity, "ok");
    }
}

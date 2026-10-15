use std::path::Path;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub const SCHEMA_SQL: &str = include_str!("schema.sql");

pub async fn open(data_dir: &Path) -> Result<SqlitePool, sqlx::Error> {
    tokio::fs::create_dir_all(data_dir).await.map_err(|e| {
        sqlx::Error::Configuration(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to create data dir: {e}"),
        )))
    })?;

    let db_path = data_dir.join("tutor.db");
    let conn_str = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());
    open_url(&conn_str).await
}

pub async fn open_url(conn_str: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(conn_str)
        .await?;

    sqlx::query("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .execute(&pool)
        .await?;

    sqlx::raw_sql(SCHEMA_SQL).execute(&pool).await?;

    Ok(pool)
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
            "questions",
            "sessions",
            "settings",
            "students",
            "subjects",
            "units",
        ];
        assert_eq!(names, expected);
    }
}

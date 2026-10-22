use sqlx::{Connection, SqliteConnection, SqlitePool};

/// 一個版本化的 schema 遷移；`version` 寫入 `PRAGMA user_version`，必須由 1 開始遞增
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// 讀取目前的 schema 版本（`PRAGMA user_version`）
pub async fn current_version(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("PRAGMA user_version").fetch_one(pool).await
}

/// 在單一交易中依序套用所有尚未執行的遷移；任一步失敗或最後留下外鍵違規，就整批回滾。
/// 外鍵只在最後檢查一次，因為 v1 基準可能接收到既有的髒資料，要等到後續遷移清理。
pub async fn run(pool: &SqlitePool, migrations: &[Migration]) -> Result<(), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    let current: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;

    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current).collect();
    if pending.is_empty() {
        return Ok(());
    }

    // 依 SQLite 官方重建資料表流程：foreign_keys 不能在交易中切換，必須在 BEGIN 之前關閉
    sqlx::raw_sql("PRAGMA foreign_keys = OFF").execute(&mut *conn).await?;
    let result = apply_in_tx(&mut conn, &pending).await;
    sqlx::raw_sql("PRAGMA foreign_keys = ON").execute(&mut *conn).await?;
    result
}

async fn apply_in_tx(conn: &mut SqliteConnection, pending: &[&Migration]) -> Result<(), sqlx::Error> {
    let mut tx = conn.begin().await?;

    for m in pending {
        if let Err(e) = sqlx::raw_sql(m.sql).execute(&mut *tx).await {
            tx.rollback().await?;
            return Err(sqlx::Error::Protocol(format!(
                "migration {:04}_{} failed: {e}",
                m.version, m.name
            )));
        }
    }

    let violations: Vec<(String, Option<i64>, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut *tx)
            .await?;
    let last = pending[pending.len() - 1];
    if !violations.is_empty() {
        tx.rollback().await?;
        let sample: Vec<String> = violations
            .iter()
            .take(5)
            .map(|(table, rowid, parent, _)| format!("{table}#{} -> {parent}", rowid.unwrap_or(-1)))
            .collect();
        return Err(sqlx::Error::Protocol(format!(
            "migrating to v{} left {} foreign key violation(s): {}",
            last.version,
            violations.len(),
            sample.join(", ")
        )));
    }

    sqlx::raw_sql(&format!("PRAGMA user_version = {}", last.version))
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIGRATIONS: &[Migration] = &[
        Migration { version: 1, name: "a", sql: "CREATE TABLE p (id INTEGER PRIMARY KEY);" },
        Migration {
            version: 2,
            name: "b",
            sql: "CREATE TABLE c (id INTEGER PRIMARY KEY, p_id INTEGER REFERENCES p(id));",
        },
    ];

    async fn memory_pool() -> SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_run_is_idempotent_and_sets_version() {
        let pool = memory_pool().await;
        run(&pool, MIGRATIONS).await.unwrap();
        run(&pool, MIGRATIONS).await.unwrap();
        assert_eq!(current_version(&pool).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_fk_violation_rolls_back() {
        let pool = memory_pool().await;
        run(&pool, &MIGRATIONS[..1]).await.unwrap();

        let bad = [Migration {
            version: 2,
            name: "bad",
            sql: "CREATE TABLE c (id INTEGER PRIMARY KEY, p_id INTEGER REFERENCES p(id));
                  INSERT INTO c VALUES (1, 42);",
        }];
        let err = run(&pool, &bad).await.unwrap_err();
        assert!(err.to_string().contains("foreign key violation"), "{err}");
        assert_eq!(current_version(&pool).await.unwrap(), 1);

        let tables: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE name = 'c'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(tables.is_empty());

        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&pool).await.unwrap();
        assert_eq!(fk, 1);
    }
}

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use axum::{middleware, Router};
use dashmap::DashMap;
use sqlx::SqlitePool;
use tokio::sync::Semaphore;

use crate::tutor::ai::Backends;

pub mod shared;
pub mod domain;
pub mod store;
pub mod ai;
pub mod features;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct TutorState {
    pub pool: SqlitePool,
    pub data_dir: PathBuf,
    pub ai: Arc<Backends>,
    pub ai_sem: Arc<Semaphore>,
    pub session_locks: Arc<DashMap<String, ()>>,
}

pub async fn init(data_dir: PathBuf, api_key: Arc<String>) -> Result<(TutorState, Router), Box<dyn std::error::Error>> {
    let pool = shared::db::open(&data_dir).await?;

    // Auto import if questions table is empty
    let q_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM questions")
        .fetch_one(&pool)
        .await?;

    if q_count.0 == 0 {
        let import_root = std::env::var("TUTOR_IMPORT_ROOT")
            .unwrap_or_else(|_| "C:/workspace/jamie-exam".to_string());
        let _ = features::bank_import::import_all(&pool, std::path::Path::new(&import_root), &data_dir).await;
    }

    // Clean runs older than 7 days
    let runs_dir = data_dir.join("runs");
    if runs_dir.exists() {
        if let Ok(mut entries) = tokio::fs::read_dir(&runs_dir).await {
            let cutoff = std::time::SystemTime::now() - Duration::from_secs(7 * 24 * 3600);
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    if let Ok(mod_time) = meta.modified() {
                        if mod_time < cutoff {
                            let _ = tokio::fs::remove_dir_all(entry.path()).await;
                        }
                    }
                }
            }
        }
    }

    let ai = Arc::new(Backends::init(pool.clone(), data_dir.clone()).await);

    let concurrency = std::env::var("TUTOR_AI_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let state = TutorState {
        pool,
        data_dir,
        ai,
        ai_sem: Arc::new(Semaphore::new(concurrency)),
        session_locks: Arc::new(DashMap::new()),
    };

    let router = routes(state.clone(), api_key);
    Ok((state, router))
}

/// 學童端路由開放使用；`/admin/*`（換 AI 後端、呼叫 CLI、改題庫）需 API Key。
pub fn routes(state: TutorState, api_key: Arc<String>) -> Router {
    let admin = Router::new()
        .merge(features::bank_import::routes())
        .merge(features::bank_admin::routes())
        .merge(features::ai_admin::routes())
        .merge(features::bank_enrich::routes())
        .route_layer(middleware::from_fn_with_state(
            api_key,
            crate::api::auth::require_api_key,
        ));

    Router::new()
        .merge(features::students::routes())
        .merge(features::catalog::routes())
        .merge(features::assets::routes())
        .merge(features::start_session::routes())
        .merge(features::session_nav::routes())
        .merge(features::answer::routes())
        .merge(features::ask_tutor::routes())
        .merge(features::report::routes())
        .merge(admin)
        .with_state(state)
}

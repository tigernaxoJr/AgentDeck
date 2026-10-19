use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use chrono::Utc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;

use ai_cli::{AiEngine, BackendConfig, Registry};

pub struct Backends {
    pub pool: SqlitePool,
    pub data_dir: PathBuf,
    pub engines: RwLock<Vec<Arc<dyn AiEngine>>>,
    pub vision_engines: RwLock<Vec<Arc<dyn AiEngine>>>,
}

impl Backends {
    pub async fn init(pool: SqlitePool, data_dir: PathBuf) -> Self {
        let b = Self {
            pool,
            data_dir,
            engines: RwLock::new(Vec::new()),
            vision_engines: RwLock::new(Vec::new()),
        };
        b.reload().await;
        b
    }

    pub async fn reload(&self) {
        let (active_ids, vision_ids) = self.load_backend_ids().await;
        let runs_dir = self.data_dir.join("runs");
        let toml_path = self.data_dir.join("ai_backends.toml");

        let mut program_overrides = HashMap::new();
        let mut model_overrides = HashMap::new();

        for id in ["agy", "claude", "gemini", "codex"] {
            let env_bin_key = format!("TUTOR_CLI_BIN_{}", id.to_uppercase());
            if let Ok(val) = std::env::var(&env_bin_key)
                && !val.is_empty() {
                    program_overrides.insert(id.to_string(), val);
                }
            let env_model_key = format!("TUTOR_MODEL_{}", id.to_uppercase());
            if let Ok(val) = std::env::var(&env_model_key)
                && !val.is_empty() {
                    model_overrides.insert(id.to_string(), val);
                }
        }

        let main_cfg = BackendConfig {
            active_ids: active_ids.clone(),
            program_overrides: program_overrides.clone(),
            model_overrides: model_overrides.clone(),
            toml_path: Some(toml_path.clone()),
            runs_dir: runs_dir.clone(),
        };
        let main_engines = Registry::build(&main_cfg);

        // Filter out unhealthy (NotInstalled)
        let mut healthy_main = Vec::new();
        for eng in main_engines {
            if let Err(ai_cli::AiError::NotInstalled(_)) = eng.health_check().await {
                continue;
            }
            healthy_main.push(eng);
        }

        let vision_cfg = BackendConfig {
            active_ids: vision_ids,
            program_overrides,
            model_overrides,
            toml_path: Some(toml_path),
            runs_dir,
        };
        let vision_engines = Registry::build(&vision_cfg);
        let mut healthy_vision = Vec::new();
        for eng in vision_engines {
            if let Err(ai_cli::AiError::NotInstalled(_)) = eng.health_check().await {
                continue;
            }
            if eng.capabilities().vision != ai_cli::VisionMode::None {
                healthy_vision.push(eng);
            }
        }

        *self.engines.write().await = healthy_main;
        *self.vision_engines.write().await = healthy_vision;
    }

    async fn load_backend_ids(&self) -> (Vec<String>, Vec<String>) {
        let db_active = self.get_setting("ai_backends").await;
        let db_vision = self.get_setting("ai_backends_vision").await;

        let active_str = db_active
            .or_else(|| std::env::var("TUTOR_AI_BACKENDS").ok())
            .unwrap_or_else(|| "agy".to_string());

        let vision_str = db_vision
            .or_else(|| std::env::var("TUTOR_AI_BACKENDS_VISION").ok())
            .unwrap_or_else(|| active_str.clone());

        let active: Vec<String> = active_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let vision: Vec<String> = vision_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        (active, vision)
    }

    pub async fn get_setting(&self, key: &str) -> Option<String> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

        row.map(|(v,)| v)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
        )
        .bind(key)
        .bind(value)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.reload().await;
        Ok(())
    }

    pub async fn engines_for(&self, purpose: ai_cli::AiPurpose) -> Vec<Arc<dyn AiEngine>> {
        match purpose {
            ai_cli::AiPurpose::GradeHandwriting | ai_cli::AiPurpose::EnrichPage => {
                self.vision_engines.read().await.clone()
            }
            _ => self.engines.read().await.clone(),
        }
    }
}

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use serde::Deserialize;

use crate::adapters::{
    generic::GenericBackendToml, AgyAdapter, ClaudeCodeAdapter, CodexAdapter, GeminiCliAdapter,
    GenericAdapter,
};
use crate::cli::CliEngine;
use crate::engine::AiEngine;

#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfigFile {
    #[serde(default)]
    pub backend: Vec<GenericBackendToml>,
}

#[derive(Debug, Clone, Default)]
pub struct BackendConfig {
    pub active_ids: Vec<String>,
    pub program_overrides: HashMap<String, String>,
    pub model_overrides: HashMap<String, String>,
    pub toml_path: Option<PathBuf>,
    pub runs_dir: PathBuf,
}

pub struct Registry;

impl Registry {
    pub fn build(cfg: &BackendConfig) -> Vec<Arc<dyn AiEngine>> {
        let mut generic_map: HashMap<String, GenericBackendToml> = HashMap::new();
        if let Some(ref p) = cfg.toml_path {
            if p.exists() {
                if let Ok(content) = std::fs::read_to_string(p) {
                    if let Ok(file_cfg) = toml::from_str::<BackendConfigFile>(&content) {
                        for b in file_cfg.backend {
                            generic_map.insert(b.id.clone(), b);
                        }
                    }
                }
            }
        }

        let mut engines: Vec<Arc<dyn AiEngine>> = Vec::new();

        for id in &cfg.active_ids {
            let prog_ov = cfg.program_overrides.get(id).cloned();
            let model_ov = cfg.model_overrides.get(id).cloned();
            let runs = cfg.runs_dir.clone();

            match id.as_str() {
                "agy" => {
                    engines.push(Arc::new(CliEngine::with_overrides(
                        AgyAdapter, prog_ov, model_ov, runs,
                    )));
                }
                "claude" => {
                    engines.push(Arc::new(CliEngine::with_overrides(
                        ClaudeCodeAdapter, prog_ov, model_ov, runs,
                    )));
                }
                "gemini" => {
                    engines.push(Arc::new(CliEngine::with_overrides(
                        GeminiCliAdapter, prog_ov, model_ov, runs,
                    )));
                }
                "codex" => {
                    engines.push(Arc::new(CliEngine::with_overrides(
                        CodexAdapter, prog_ov, model_ov, runs,
                    )));
                }
                custom_id => {
                    if let Some(generic_cfg) = generic_map.get(custom_id) {
                        engines.push(Arc::new(CliEngine::with_overrides(
                            GenericAdapter::new(generic_cfg.clone()),
                            prog_ov,
                            model_ov,
                            runs,
                        )));
                    } else {
                        eprintln!("Warning: Backend '{custom_id}' not found in built-ins or toml");
                    }
                }
            }
        }

        engines
    }
}

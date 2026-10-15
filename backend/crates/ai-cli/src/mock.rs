use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::engine::{
    AiEngine, AiError, AiInvocation, AiRawResult, Capabilities, VisionMode,
};

pub struct MockEngine {
    pub id: String,
    pub capabilities: Capabilities,
    pub model: Option<String>,
    pub results: Arc<Mutex<Vec<Result<AiRawResult, AiError>>>>,
    pub calls: Arc<Mutex<Vec<AiInvocation>>>,
    pub call_count: AtomicUsize,
}

impl MockEngine {
    pub fn new(id: &str, results: Vec<Result<AiRawResult, AiError>>) -> Self {
        Self {
            id: id.to_string(),
            capabilities: Capabilities {
                vision: VisionMode::PromptPath,
                reads_files: true,
                writes_files: true,
                max_prompt_chars: 10000,
            },
            model: None,
            results: Arc::new(Mutex::new(results)),
            calls: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn with_capabilities(
        id: &str,
        capabilities: Capabilities,
        results: Vec<Result<AiRawResult, AiError>>,
    ) -> Self {
        Self {
            id: id.to_string(),
            capabilities,
            model: None,
            results: Arc::new(Mutex::new(results)),
            calls: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl AiEngine for MockEngine {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn model(&self) -> Option<String> {
        self.model.clone()
    }

    async fn health_check(&self) -> Result<String, AiError> {
        Ok("mock 1.0.0".to_string())
    }

    async fn run(&self, inv: AiInvocation) -> Result<AiRawResult, AiError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut calls = self.calls.lock().await;
        calls.push(inv);

        let mut results = self.results.lock().await;
        if results.is_empty() {
            Err(AiError::EmptyOutput)
        } else {
            results.remove(0)
        }
    }
}

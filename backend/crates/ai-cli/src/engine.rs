use std::fmt;
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub vision: VisionMode,
    pub reads_files: bool,
    pub writes_files: bool,
    pub max_prompt_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisionMode {
    None,
    PromptPath,
    AtMention,
    Flag { flag: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiPurpose {
    GradeHandwriting,
    TutorWrong,
    StudentMessage,
    EnrichPage,
}

pub struct RunFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub struct AiInvocation {
    pub call_id: String,
    pub purpose: AiPurpose,
    pub prompt: String,
    pub files: Vec<RunFile>,
    pub images: Vec<String>,
    pub expect_output_file: Option<String>,
    pub model: Option<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

pub struct AiRawResult {
    pub backend_id: String,
    pub output_file: Option<String>,
    pub final_text: Option<String>,
    pub usage: Option<TokenUsage>,
    pub duration: Duration,
    pub stderr_tail: String,
}

#[derive(Debug)]
pub enum AiError {
    NotInstalled(String),
    Spawn(String),
    Timeout,
    NonZeroExit {
        code: Option<i32>,
        stderr_tail: String,
    },
    EmptyOutput,
    Unsupported(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NotInstalled(s) => write!(f, "CLI not installed: {s}"),
            AiError::Spawn(s) => write!(f, "Failed to spawn CLI: {s}"),
            AiError::Timeout => write!(f, "CLI execution timed out"),
            AiError::NonZeroExit { code, stderr_tail } => {
                write!(f, "CLI exited with code {:?}: {}", code, stderr_tail)
            }
            AiError::EmptyOutput => write!(f, "CLI gave empty output"),
            AiError::Unsupported(s) => write!(f, "Unsupported operation: {s}"),
        }
    }
}

impl std::error::Error for AiError {}

#[async_trait::async_trait]
pub trait AiEngine: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> &Capabilities;
    fn model(&self) -> Option<String>;
    async fn health_check(&self) -> Result<String, AiError>;
    async fn run(&self, inv: AiInvocation) -> Result<AiRawResult, AiError>;
}

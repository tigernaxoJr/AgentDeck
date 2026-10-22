use std::path::Path;
use serde::Deserialize;
use serde_json::Value;

use crate::cli::{CliAdapter, CommandSpec, StreamItem};
use crate::engine::{AiInvocation, Capabilities, VisionMode};

#[derive(Debug, Clone, Deserialize)]
pub struct GenericBackendToml {
    pub id: String,
    pub program: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub model_args: Vec<String>,
    #[serde(default)]
    pub image_args: Vec<String>,
    #[serde(default = "default_vision")]
    pub vision: String,
    #[serde(default = "default_true")]
    pub reads_files: bool,
    #[serde(default = "default_true")]
    pub writes_files: bool,
    #[serde(default = "default_max_prompt")]
    pub max_prompt_chars: usize,
    #[serde(default)]
    pub prompt_via_stdin: bool,
    #[serde(default = "default_line_mode")]
    pub line_mode: String,
    pub final_text_pointer: Option<String>,
    pub text_delta_pointer: Option<String>,
    #[serde(default = "default_version_args")]
    pub version_args: Vec<String>,
}

fn default_vision() -> String { "none".into() }
fn default_true() -> bool { true }
fn default_max_prompt() -> usize { 30000 }
fn default_line_mode() -> String { "text".into() }
fn default_version_args() -> Vec<String> { vec!["--version".into()] }

pub struct GenericAdapter {
    pub config: GenericBackendToml,
}

impl GenericAdapter {
    pub fn new(config: GenericBackendToml) -> Self {
        Self { config }
    }
}

impl CliAdapter for GenericAdapter {
    fn id(&self) -> &'static str {
        Box::leak(self.config.id.clone().into_boxed_str())
    }

    fn default_program(&self) -> String {
        self.config.program.clone()
    }

    fn capabilities(&self) -> Capabilities {
        let vision = match self.config.vision.to_lowercase().as_str() {
            "prompt_path" => VisionMode::PromptPath,
            "at_mention" => VisionMode::AtMention,
            "flag" => VisionMode::Flag {
                flag: self.config.image_args.first().cloned().unwrap_or_else(|| "-i".into()),
            },
            _ => VisionMode::None,
        };

        Capabilities {
            vision,
            reads_files: self.config.reads_files,
            writes_files: self.config.writes_files,
            max_prompt_chars: self.config.max_prompt_chars,
        }
    }

    fn version_args(&self) -> Vec<String> {
        self.config.version_args.clone()
    }

    fn build_command(&self, program: &str, inv: &AiInvocation, _run_dir: &Path) -> CommandSpec {
        let mut final_args = Vec::new();
        let prompt_str = &inv.prompt;
        let model_str = inv.model.as_deref().unwrap_or("");

        // `{extra_args}` 決定額外旗標的位置；樣板沒寫就附加在最後
        let mut extra_placed = false;
        for arg in &self.config.args {
            if arg == "{extra_args}" {
                final_args.extend(inv.extra_args.iter().cloned());
                extra_placed = true;
            } else if arg == "{model_args}" {
                if !model_str.is_empty() {
                    for m_arg in &self.config.model_args {
                        final_args.push(m_arg.replace("{model}", model_str));
                    }
                }
            } else if arg == "{image_args}" {
                for img in &inv.images {
                    for i_arg in &self.config.image_args {
                        final_args.push(i_arg.replace("{image}", img));
                    }
                }
            } else {
                let replaced = arg
                    .replace("{prompt}", prompt_str)
                    .replace("{model}", model_str);
                final_args.push(replaced);
            }
        }

        if !extra_placed {
            final_args.extend(inv.extra_args.iter().cloned());
        }

        let stdin = if self.config.prompt_via_stdin {
            Some(inv.prompt.clone())
        } else {
            None
        };

        CommandSpec {
            program: program.to_string(),
            args: final_args,
            env: Vec::new(),
            stdin,
        }
    }

    fn parse_line(&self, line: &str) -> StreamItem {
        if self.config.line_mode == "ndjson" {
            if let Ok(val) = serde_json::from_str::<Value>(line) {
                if let Some(ref ptr) = self.config.final_text_pointer
                    && let Some(target) = val.pointer(ptr)
                        && let Some(s) = target.as_str() {
                            return StreamItem::FinalText(s.to_string());
                        }
                if let Some(ref ptr) = self.config.text_delta_pointer
                    && let Some(target) = val.pointer(ptr)
                        && let Some(s) = target.as_str() {
                            return StreamItem::TextDelta(s.to_string());
                        }
            }
            StreamItem::Ignore
        } else {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                StreamItem::TextDelta(format!("{}\n", line))
            } else {
                StreamItem::Ignore
            }
        }
    }
}

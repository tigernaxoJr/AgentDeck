use std::path::Path;
use serde_json::Value;

use crate::cli::{CliAdapter, CommandSpec, StreamItem};
use crate::engine::{AiInvocation, Capabilities, TokenUsage, VisionMode};

pub struct ClaudeCodeAdapter;

impl CliAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn default_program(&self) -> String {
        "claude".to_string()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vision: VisionMode::PromptPath,
            reads_files: true,
            writes_files: true,
            max_prompt_chars: 30000,
        }
    }

    fn build_command(&self, program: &str, inv: &AiInvocation, _run_dir: &Path) -> CommandSpec {
        let mut args = vec![
            "-p".to_string(),
            inv.prompt.clone(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
            "--dangerously-skip-permissions".to_string(),
        ];

        if let Some(ref m) = inv.model {
            args.push("--model".to_string());
            args.push(m.clone());
        }
        args.extend(inv.extra_args.iter().cloned());

        CommandSpec {
            program: program.to_string(),
            args,
            env: Vec::new(),
            stdin: None,
        }
    }

    fn parse_line(&self, line: &str) -> StreamItem {
        let v: Value = match serde_json::from_str(line) {
            Ok(val) => val,
            Err(_) => return StreamItem::Ignore,
        };

        let typ = v.get("type").and_then(|t| t.as_str());

        if typ == Some("result")
            && let Some(res) = v.get("result").and_then(|r| r.as_str()) {
                return StreamItem::FinalText(res.to_string());
            }

        if let Some(usage_val) = v.get("usage") {
            return StreamItem::Usage(TokenUsage {
                input_tokens: usage_val.get("input_tokens").and_then(|n| n.as_i64()),
                output_tokens: usage_val.get("output_tokens").and_then(|n| n.as_i64()),
                total_tokens: None,
            });
        }

        StreamItem::Ignore
    }
}

pub struct GeminiCliAdapter;

impl CliAdapter for GeminiCliAdapter {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn default_program(&self) -> String {
        if cfg!(windows) {
            "gemini.cmd".to_string()
        } else {
            "gemini".to_string()
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vision: VisionMode::AtMention,
            reads_files: true,
            writes_files: true,
            max_prompt_chars: 30000,
        }
    }

    fn build_command(&self, program: &str, inv: &AiInvocation, _run_dir: &Path) -> CommandSpec {
        let mut args = vec![
            "-p".to_string(),
            inv.prompt.clone(),
            "-y".to_string(),
        ];

        if let Some(ref m) = inv.model {
            args.push("-m".to_string());
            args.push(m.clone());
        }
        args.extend(inv.extra_args.iter().cloned());

        CommandSpec {
            program: program.to_string(),
            args,
            env: Vec::new(),
            stdin: None,
        }
    }

    fn parse_line(&self, line: &str) -> StreamItem {
        if let Ok(v) = serde_json::from_str::<Value>(line)
            && let Some(resp) = v.get("response").and_then(|r| r.as_str()) {
                return StreamItem::FinalText(resp.to_string());
            }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            StreamItem::TextDelta(format!("{}\n", line))
        } else {
            StreamItem::Ignore
        }
    }
}

pub struct CodexAdapter;

impl CliAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn default_program(&self) -> String {
        "codex".to_string()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vision: VisionMode::Flag {
                flag: "-i".to_string(),
            },
            reads_files: true,
            writes_files: true,
            max_prompt_chars: 30000,
        }
    }

    fn build_command(&self, program: &str, inv: &AiInvocation, _run_dir: &Path) -> CommandSpec {
        let mut args = vec![
            "exec".to_string(),
            "--full-auto".to_string(),
            "--skip-git-repo-check".to_string(),
        ];

        if let Some(ref m) = inv.model {
            args.push("-m".to_string());
            args.push(m.clone());
        }

        for img in &inv.images {
            args.push("-i".to_string());
            args.push(img.clone());
        }

        args.extend(inv.extra_args.iter().cloned());
        args.push(inv.prompt.clone());

        CommandSpec {
            program: program.to_string(),
            args,
            env: Vec::new(),
            stdin: None,
        }
    }

    fn parse_line(&self, line: &str) -> StreamItem {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            StreamItem::TextDelta(format!("{}\n", line))
        } else {
            StreamItem::Ignore
        }
    }
}

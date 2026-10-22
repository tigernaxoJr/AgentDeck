use std::path::Path;
use serde_json::Value;

use crate::cli::{which, CliAdapter, CommandSpec, StreamItem};
use crate::engine::{AiInvocation, Capabilities, TokenUsage, VisionMode};

pub fn resolve_agy_bin() -> String {
    std::env::var("ANTIGRAVITY_CLI_BIN").unwrap_or_else(|_| {
        if which("agy") {
            "agy".to_string()
        } else {
            "antigravity".to_string()
        }
    })
}

pub struct AgyAdapter;

impl CliAdapter for AgyAdapter {
    fn id(&self) -> &'static str {
        "agy"
    }

    fn default_program(&self) -> String {
        resolve_agy_bin()
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
            "--dangerously-skip-permissions".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];

        if let Some(ref m) = inv.model {
            args.push("--model".to_string());
            args.push(m.clone());
        }

        args.extend(inv.extra_args.iter().cloned());
        args.push("-p".to_string());
        args.push(inv.prompt.clone());

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

        let event = v.get("event").and_then(|e| e.as_str());

        if event == Some("step_update") {
            if let Some(su) = v.get("step_update") {
                if let Some(delta) = su.get("text_delta").and_then(|d| d.as_str()) {
                    return StreamItem::TextDelta(delta.to_string());
                }
                if let Some(usage_val) = su.get("usage") {
                    return StreamItem::Usage(parse_usage(usage_val));
                }
            }
        } else if event == Some("result")
            && let Some(res) = v.get("result") {
                if let Some(usage_val) = res.get("usage") {
                    // Could emit usage or final text
                    let final_txt = res.get("response").and_then(|r| r.as_str()).map(|s| s.to_string());
                    if let Some(txt) = final_txt {
                        return StreamItem::FinalText(txt);
                    }
                    return StreamItem::Usage(parse_usage(usage_val));
                }
                if let Some(resp) = res.get("response").and_then(|r| r.as_str()) {
                    return StreamItem::FinalText(resp.to_string());
                }
            }

        // Direct /usage fallback
        if let Some(usage_val) = v.get("usage") {
            return StreamItem::Usage(parse_usage(usage_val));
        }

        StreamItem::Ignore
    }
}

fn parse_usage(v: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: v.get("input_tokens").and_then(|n| n.as_i64()),
        output_tokens: v.get("output_tokens").and_then(|n| n.as_i64()),
        total_tokens: v.get("total_tokens").and_then(|n| n.as_i64()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AiPurpose;
    use std::time::Duration;

    #[test]
    fn test_agy_build_command() {
        let adapter = AgyAdapter;
        let inv = AiInvocation {
            call_id: "test_call".into(),
            purpose: AiPurpose::TutorWrong,
            prompt: "hello prompt".into(),
            files: vec![],
            images: vec![],
            expect_output_file: None,
            model: Some("custom-model".into()),
            timeout: Duration::from_secs(30),
            extra_args: vec!["--sandbox".into()],
        };

        let spec = adapter.build_command("agy.exe", &inv, Path::new("."));
        assert_eq!(spec.program, "agy.exe");
        assert_eq!(
            spec.args,
            vec![
                "--dangerously-skip-permissions",
                "--output-format",
                "stream-json",
                "--model",
                "custom-model",
                "--sandbox",
                "-p",
                "hello prompt"
            ]
        );
    }

    #[test]
    fn test_agy_parse_stream_fixture() {
        let adapter = AgyAdapter;
        let fixture_path = Path::new("tests/fixtures/agy_stream.txt");
        if fixture_path.exists() {
            let content = std::fs::read_to_string(fixture_path).expect("read agy fixture");
            let mut got_delta = false;
            let mut got_final = false;
            for line in content.lines() {
                match adapter.parse_line(line) {
                    StreamItem::TextDelta(_) => got_delta = true,
                    StreamItem::FinalText(_) => got_final = true,
                    _ => {}
                }
            }
            assert!(got_delta || got_final, "Expected deltas or final text in fixture");
        }
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use serde::de::DeserializeOwned;

use crate::engine::{
    AiEngine, AiError, AiInvocation, AiPurpose, Capabilities, RunFile, TokenUsage,
    VisionMode,
};

static CALL_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct CallRecord {
    pub backend: String,
    pub model: Option<String>,
    pub duration: Duration,
    pub usage: Option<TokenUsage>,
    pub ok: bool,
    pub error: Option<String>,
    pub raw_tail: String,
    pub prompt: String,
}

pub struct CallSpec<'a> {
    pub purpose: AiPurpose,
    pub needs_vision: bool,
    pub build_prompt: &'a (dyn Fn(&Capabilities) -> String + Send + Sync),
    pub files: Vec<RunFile>,
    pub images: Vec<String>,
    pub output_file: &'a str,
    pub timeout: Duration,
}

pub async fn call_json<T: DeserializeOwned>(
    engines: &[Arc<dyn AiEngine>],
    spec: CallSpec<'_>,
) -> (Result<T, AiError>, Vec<CallRecord>) {
    let mut records = Vec::new();

    for engine in engines {
        let caps = engine.capabilities();
        if spec.needs_vision && caps.vision == VisionMode::None {
            continue;
        }

        // Try engine, with 1 retry on JSON parse failure
        let mut retry_count = 0;
        let mut parse_hint = String::new();

        while retry_count < 2 {
            let mut prompt = (spec.build_prompt)(caps);
            if !parse_hint.is_empty() {
                prompt.push_str("\n\n");
                prompt.push_str(&parse_hint);
            }

            let count = CALL_COUNTER.fetch_add(1, Ordering::SeqCst);
            let call_id = format!("call_{}_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), count);
            let mut run_files = Vec::new();
            for f in &spec.files {
                run_files.push(RunFile {
                    name: f.name.clone(),
                    bytes: f.bytes.clone(),
                });
            }

            let inv = AiInvocation {
                call_id,
                purpose: spec.purpose,
                prompt: prompt.clone(),
                files: run_files,
                images: spec.images.clone(),
                expect_output_file: if caps.writes_files {
                    Some(spec.output_file.to_string())
                } else {
                    None
                },
                model: engine.model(),
                timeout: spec.timeout,
            };

            let res = engine.run(inv).await;
            match res {
                Ok(raw) => {
                    let text_to_parse = raw
                        .output_file
                        .as_deref()
                        .or(raw.final_text.as_deref())
                        .unwrap_or_default();

                    let parsed = parse_json_from_str::<T>(text_to_parse);
                    match parsed {
                        Ok(val) => {
                            records.push(CallRecord {
                                backend: raw.backend_id,
                                model: engine.model(),
                                duration: raw.duration,
                                usage: raw.usage,
                                ok: true,
                                error: None,
                                raw_tail: text_to_parse.chars().take(4096).collect(),
                                prompt: prompt.clone(),
                            });
                            return (Ok(val), records);
                        }
                        Err(parse_err) => {
                            records.push(CallRecord {
                                backend: raw.backend_id.clone(),
                                model: engine.model(),
                                duration: raw.duration,
                                usage: raw.usage,
                                ok: false,
                                error: Some(format!("JSON parse error: {parse_err}")),
                                raw_tail: text_to_parse.chars().take(4096).collect(),
                                prompt: prompt.clone(),
                            });
                            retry_count += 1;
                            parse_hint = "上次輸出不是合法 JSON，請只輸出 JSON 物件，不要有任何其他文字。".to_string();
                        }
                    }
                }
                Err(e) => {
                    records.push(CallRecord {
                        backend: engine.id().to_string(),
                        model: engine.model(),
                        duration: Duration::from_millis(0),
                        usage: None,
                        ok: false,
                        error: Some(e.to_string()),
                        raw_tail: String::new(),
                        prompt: prompt.clone(),
                    });
                    break; // Switch to next engine
                }
            }
        }
    }

    (Err(AiError::EmptyOutput), records)
}

fn parse_json_from_str<T: DeserializeOwned>(s: &str) -> Result<T, serde_json::Error> {
    if let Ok(val) = serde_json::from_str::<T>(s) {
        return Ok(val);
    }
    // Extract substring between first '{' and last '}'
    if let (Some(start), Some(end)) = (s.find('{'), s.rfind('}')) {
        if start < end {
            let slice = &s[start..=end];
            return serde_json::from_str::<T>(slice);
        }
    }
    serde_json::from_str::<T>(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockEngine;
    use crate::engine::AiRawResult;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Deserialize)]
    struct DummyOut {
        value: String,
    }

    #[tokio::test]
    async fn test_call_json_fallback_and_retry() {
        let engine1 = Arc::new(MockEngine::new(
            "mock1",
            vec![
                Err(AiError::Timeout),
            ],
        ));
        let engine2 = Arc::new(MockEngine::new(
            "mock2",
            vec![
                Ok(AiRawResult {
                    backend_id: "mock2".into(),
                    output_file: Some("not json".into()),
                    final_text: None,
                    usage: None,
                    duration: Duration::from_millis(100),
                    stderr_tail: "".into(),
                }),
                Ok(AiRawResult {
                    backend_id: "mock2".into(),
                    output_file: Some(r#"{"value": "hello"}"#.into()),
                    final_text: None,
                    usage: None,
                    duration: Duration::from_millis(150),
                    stderr_tail: "".into(),
                }),
            ],
        ));

        let engines: Vec<Arc<dyn AiEngine>> = vec![engine1, engine2];
        let prompt_builder = |_caps: &Capabilities| "test prompt".to_string();

        let spec = CallSpec {
            purpose: AiPurpose::TutorWrong,
            needs_vision: false,
            build_prompt: &prompt_builder,
            files: vec![],
            images: vec![],
            output_file: "out/turn.json",
            timeout: Duration::from_secs(5),
        };

        let (res, records) = call_json::<DummyOut>(&engines, spec).await;
        let out = res.expect("call_json should succeed on engine 2 retry");
        assert_eq!(out.value, "hello");
        assert_eq!(records.len(), 3); // 1 timeout from engine1, 1 parse error from engine2, 1 success from engine2
        assert!(!records[0].ok);
        assert!(!records[1].ok);
        assert!(records[2].ok);
    }

    #[tokio::test]
    async fn test_call_json_skips_non_vision() {
        let engine_novision = Arc::new(MockEngine::with_capabilities(
            "novision",
            Capabilities {
                vision: VisionMode::None,
                reads_files: true,
                writes_files: true,
                max_prompt_chars: 1000,
            },
            vec![Ok(AiRawResult {
                backend_id: "novision".into(),
                output_file: Some(r#"{"value": "wrong"}"#.into()),
                final_text: None,
                usage: None,
                duration: Duration::from_millis(50),
                stderr_tail: "".into(),
            })],
        ));

        let engine_vision = Arc::new(MockEngine::with_capabilities(
            "vision",
            Capabilities {
                vision: VisionMode::PromptPath,
                reads_files: true,
                writes_files: true,
                max_prompt_chars: 1000,
            },
            vec![Ok(AiRawResult {
                backend_id: "vision".into(),
                output_file: Some(r#"{"value": "vision_ok"}"#.into()),
                final_text: None,
                usage: None,
                duration: Duration::from_millis(50),
                stderr_tail: "".into(),
            })],
        ));

        let engines: Vec<Arc<dyn AiEngine>> = vec![engine_novision, engine_vision];
        let prompt_builder = |_caps: &Capabilities| "test prompt".to_string();

        let spec = CallSpec {
            purpose: AiPurpose::GradeHandwriting,
            needs_vision: true,
            build_prompt: &prompt_builder,
            files: vec![],
            images: vec!["answer.png".into()],
            output_file: "out/turn.json",
            timeout: Duration::from_secs(5),
        };

        let (res, records) = call_json::<DummyOut>(&engines, spec).await;
        let out = res.expect("call_json should pick vision engine");
        assert_eq!(out.value, "vision_ok");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].backend, "vision");
    }
}

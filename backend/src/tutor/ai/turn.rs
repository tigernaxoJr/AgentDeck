use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use ai_cli::{call_json, AiEngine, CallSpec, RunFile};
use crate::tutor::ai::prompts::{build_prompt, PromptContext};
use crate::tutor::domain::question::{contains_answer, Verdict};
use crate::tutor::shared::dto::BookRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTurn {
    #[serde(default = "default_verdict")]
    pub verdict: String,
    #[serde(default)]
    pub recognized_text: String,
    #[serde(default)]
    pub say: String,
    #[serde(default = "default_action")]
    pub action: String,
    pub book_ref: Option<AiBookRef>,
    #[serde(default)]
    pub misconception: String,
    #[serde(default)]
    pub concept_tags: Vec<String>,
    pub followup_question: Option<String>,
}

fn default_verdict() -> String { "none".into() }
fn default_action() -> String { "explain".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiBookRef {
    pub page: i64,
    pub where_clause: Option<String>,
    #[serde(rename = "where")]
    pub where_alias: Option<String>,
}

impl AiBookRef {
    pub fn get_where(&self) -> String {
        self.where_clause
            .clone()
            .or_else(|| self.where_alias.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct TutorFeedback {
    pub verdict: Verdict,
    pub say: String,
    pub action: String,
    pub book_ref: Option<BookRef>,
    pub recognized_text: String,
    pub misconception: Option<String>,
    pub concept_tags: Vec<String>,
    pub followup_question: Option<String>,
}

#[derive(Debug)]
pub enum TurnError {
    NoBackend,
    ExecutionFailed(String),
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TurnError::NoBackend => write!(f, "No AI backend available"),
            TurnError::ExecutionFailed(e) => write!(f, "AI execution failed: {e}"),
        }
    }
}

impl std::error::Error for TurnError {}

pub async fn call_turn(
    engines: &[Arc<dyn AiEngine>],
    purpose: ai_cli::AiPurpose,
    ctx: &PromptContext,
    files: Vec<RunFile>,
    images: Vec<String>,
    pool: &SqlitePool,
    session_id: Option<&str>,
    timeout: Duration,
) -> Result<TutorFeedback, TurnError> {
    if engines.is_empty() {
        return Err(TurnError::NoBackend);
    }

    let prompt_builder = |caps: &ai_cli::Capabilities| build_prompt(purpose, ctx, caps);
    let needs_vision = matches!(purpose, ai_cli::AiPurpose::GradeHandwriting | ai_cli::AiPurpose::EnrichPage);

    let spec = CallSpec {
        purpose,
        needs_vision,
        build_prompt: &prompt_builder,
        files,
        images,
        output_file: "out/turn.json",
        timeout,
    };

    let (res, records) = call_json::<AiTurn>(engines, spec).await;

    // Log records to ai_calls
    let now = Utc::now().to_rfc3339();
    let q_id = ctx.question.id.clone();
    for rec in &records {
        let call_id = uuid::Uuid::new_v4().to_string();
        let dur = rec.duration.as_millis() as i64;
        let ok_int = if rec.ok { 1 } else { 0 };
        let purpose_str = match purpose {
            ai_cli::AiPurpose::GradeHandwriting => "grade_handwriting",
            ai_cli::AiPurpose::TutorWrong => "tutor_wrong",
            ai_cli::AiPurpose::StudentMessage => "student_message",
            ai_cli::AiPurpose::EnrichPage => "enrich_page",
        };

        let inp_tok = rec.usage.as_ref().and_then(|u| u.input_tokens);
        let out_tok = rec.usage.as_ref().and_then(|u| u.output_tokens);

        let _ = sqlx::query(
            r#"
            INSERT INTO ai_calls (
                id, backend, model, purpose, session_id, question_id,
                duration_ms, ok, error, raw_output, prompt, input_tokens, output_tokens, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&call_id)
        .bind(&rec.backend)
        .bind(&rec.model)
        .bind(purpose_str)
        .bind(session_id)
        .bind(&q_id)
        .bind(dur)
        .bind(ok_int)
        .bind(&rec.error)
        .bind(&rec.raw_tail)
        .bind(&rec.prompt)
        .bind(inp_tok)
        .bind(out_tok)
        .bind(&now)
        .execute(pool)
        .await;
    }

    let mut turn = match res {
        Ok(t) => t,
        Err(e) => return Err(TurnError::ExecutionFailed(e.to_string())),
    };

    let is_correct = turn.verdict.eq_ignore_ascii_case("correct");

    // Guard 1: Leaking answer check (only applies if student has NOT answered correctly yet)
    if !is_correct && ctx.try_no <= 1 && contains_answer(&ctx.question, &turn.say) {
        turn.say = "再想想看，可以翻翻課本喔！".to_string();
    }

    // Guard 2: Book page check
    let mut safe_book_ref = None;
    if let Some(ref br) = turn.book_ref {
        let expected_page = ctx.question.book_page.unwrap_or(br.page);
        let page = if br.page == expected_page {
            br.page
        } else {
            expected_page
        };
        safe_book_ref = Some(BookRef {
            page,
            where_clause: br.get_where(),
        });
    }

    let verdict = match turn.verdict.to_lowercase().as_str() {
        "correct" => Verdict::Correct,
        "partial" => Verdict::Partial,
        "unreadable" => Verdict::Unreadable,
        _ => Verdict::Incorrect,
    };

    let misconception = if turn.misconception.trim().is_empty() {
        None
    } else {
        Some(turn.misconception.trim().to_string())
    };

    Ok(TutorFeedback {
        verdict,
        say: turn.say,
        action: turn.action,
        book_ref: safe_book_ref,
        recognized_text: turn.recognized_text,
        misconception,
        concept_tags: turn.concept_tags,
        followup_question: turn.followup_question,
    })
}

pub fn canned_praise() -> &'static str {
    let praises = [
        "答對了！好棒 🎉",
        "沒錯！你真厲害！",
        "完全正確，繼續加油！",
    ];
    let idx = (Utc::now().timestamp_millis() as usize) % praises.len();
    praises[idx]
}

pub fn canned_wrong_hint(book_ref: Option<&BookRef>) -> String {
    if let Some(br) = book_ref {
        format!("差一點點！翻到課本第 {} 頁{}看看，再想想看。", br.page, br.where_clause)
    } else {
        "差一點點！再仔細想想看喔。".to_string()
    }
}

pub fn canned_wrong_explain(answer_display: &str, explanation: &str) -> String {
    format!("我們一起看答案：{}。{}", answer_display, explanation)
}

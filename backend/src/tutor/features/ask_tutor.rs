use std::time::Duration;
use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;

use ai_cli::{AiPurpose, RunFile};
use crate::tutor::ai::build_prompt_context;
use crate::tutor::ai::turn::call_turn;
use crate::tutor::shared::dto::TutorReply;
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{insert_dialog_turn, load_question, load_session};
use crate::tutor::TutorState;
use crate::tutor::shared::enum_tag;

#[derive(Debug, Deserialize)]
pub struct AskTutorReq {
    pub question_id: String,
    pub text: String,
}

pub fn routes() -> Router<TutorState> {
    Router::new().route("/sessions/{id}/message", post(ask_tutor))
}

async fn ask_tutor(
    State(state): State<TutorState>,
    Path(id): Path<String>,
    Json(payload): Json<AskTutorReq>,
) -> Result<Json<TutorReply>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let session = load_session(&mut *conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    let q = load_question(&mut *conn, &payload.question_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;

    let now = Utc::now();

    // 1. 先記下學童的提問，再組上下文（含這句話）
    insert_dialog_turn(&mut conn, &id, &q.id, "student", payload.text.trim(), "ask_question", None, now)
        .await?;

    let ctx = build_prompt_context(&mut conn, &session, &q, session.q_try, None, Some(payload.text)).await?;

    let engines = state.ai.engines_for(AiPurpose::StudentMessage).await;
    let mut say = String::new();
    let mut book_ref = None;
    let mut action = "answer_question".to_string();

    if !engines.is_empty() {
        let run_files = vec![
            RunFile {
                name: "context.json".into(),
                bytes: crate::tutor::ai::prompts::build_context_json(&ctx).into_bytes(),
            },
        ];
        let _permit = state.ai_sem.acquire().await.map_err(|e| ApiError::Internal(e.to_string()))?;
        if let Ok(fb) = call_turn(
            &engines,
            AiPurpose::StudentMessage,
            &ctx,
            run_files,
            vec![],
            &state.pool,
            Some(&session.id),
            Duration::from_secs(90),
        ).await {
            say = fb.say;
            book_ref = fb.book_ref;
            action = fb.action;
        }
    }

    if say.is_empty() {
        say = "這題我們先看清楚題目問的是什麼，也可以翻課本參考喔！".to_string();
    }

    // 3. 記下家教回覆
    insert_dialog_turn(&mut conn, &id, &q.id, "tutor", &say, &action, book_ref.as_ref(), Utc::now())
        .await?;

    let q_state_str = enum_tag(&session.q_state);

    Ok(Json(TutorReply {
        verdict: "none".into(),
        say,
        action,
        book_ref,
        recognized_text: None,
        followup_question: None,
        q_state: q_state_str,
        reveal: None,
        can_next: session.can_next(),
        self_grade_available: Some(false),
        hint_pending: None,
    }))
}

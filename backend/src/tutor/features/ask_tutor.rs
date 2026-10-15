use std::time::Duration;
use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;

use ai_cli::{AiPurpose, RunFile};
use crate::tutor::ai::prompts::{PromptContext, PromptDialogItem, PromptWeakConcept};
use crate::tutor::ai::turn::call_turn;
use crate::tutor::shared::dto::{BookRef, TutorReply};
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{load_question, load_session, load_weaknesses};
use crate::tutor::TutorState;

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

    // 1. Record student message in dialog_turns
    sqlx::query(
        "INSERT INTO dialog_turns (session_id, question_id, role, content, action, created_at) VALUES (?, ?, 'student', ?, 'ask_question', ?)"
    )
    .bind(&id)
    .bind(&q.id)
    .bind(payload.text.trim())
    .bind(now.to_rfc3339())
    .execute(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    // 2. Build context
    let student_row: (String, i64) = sqlx::query_as("SELECT name, grade FROM students WHERE id = ?")
        .bind(session.student_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(ApiError::from)?;

    let sub_row: Option<(String, String)> = sqlx::query_as("SELECT name, book_title FROM subjects WHERE id = ?")
        .bind(&session.subject_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(ApiError::from)?;

    let (subject_name, book_title) = sub_row.unwrap_or_else(|| (session.subject_id.clone(), "課本".into()));

    let dialog_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM dialog_turns WHERE session_id = ? AND question_id = ? ORDER BY id ASC LIMIT 10"
    )
    .bind(&session.id)
    .bind(&q.id)
    .fetch_all(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    let dialog = dialog_rows
        .into_iter()
        .map(|(role, content)| PromptDialogItem { role, content })
        .collect();

    let weak_items = load_weaknesses(&mut *conn, session.student_id, 5)
        .await
        .map_err(ApiError::from)?;

    let weak_concepts = weak_items
        .into_iter()
        .map(|w| PromptWeakConcept {
            tag: w.concept_tag,
            score: w.score,
            note: w.last_note,
        })
        .collect();

    let ctx = PromptContext {
        student_name: student_row.0,
        student_grade: student_row.1,
        subject_name,
        book_title,
        question: q.clone(),
        try_no: session.q_try,
        dialog,
        weak_concepts,
        student_choice: None,
        student_message: Some(payload.text),
        q_state: session.q_state,
    };

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

    // 3. Record tutor response in dialog_turns
    let br_json = book_ref.as_ref().map(|b| serde_json::to_string(b).unwrap());
    sqlx::query(
        "INSERT INTO dialog_turns (session_id, question_id, role, content, action, book_ref_json, created_at) VALUES (?, ?, 'tutor', ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&q.id)
    .bind(&say)
    .bind(&action)
    .bind(br_json)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    let q_state_str = serde_json::to_string(&session.q_state).unwrap().trim_matches('"').to_string();

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
    }))
}

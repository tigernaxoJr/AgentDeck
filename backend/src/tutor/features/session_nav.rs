use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Serialize;

use crate::tutor::shared::dto::{BookRef, DialogTurnView, QuestionView, SessionView};
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{load_question, load_session, save_session};
use crate::tutor::TutorState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndSessionReport {
    pub answered: i64,
    pub first_try_correct: i64,
    pub weak_concepts: Vec<String>,
}

pub fn routes() -> Router<TutorState> {
    Router::new()
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/next", post(next_question))
        .route("/sessions/{id}/end", post(end_session))
}

async fn get_session(
    State(state): State<TutorState>,
    Path(id): Path<String>,
) -> Result<Json<SessionView>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let session = load_session(&mut *conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    let curr_qid = session.current_question_id();
    let q_view = if let Some(qid) = curr_qid {
        load_question(&mut *conn, qid)
            .await
            .map_err(ApiError::from)?
            .as_ref()
            .map(QuestionView::from)
    } else {
        None
    };

    let dialog_rows: Vec<(String, String, Option<String>)> = if let Some(qid) = curr_qid {
        sqlx::query_as(
            "SELECT role, content, book_ref_json FROM dialog_turns WHERE session_id = ? AND question_id = ? ORDER BY id ASC"
        )
        .bind(&id)
        .bind(qid)
        .fetch_all(&mut *conn)
        .await
        .map_err(ApiError::from)?
    } else {
        vec![]
    };

    let dialog = dialog_rows
        .into_iter()
        .map(|(role, content, br_json)| {
            let book_ref = br_json.and_then(|j| serde_json::from_str::<BookRef>(&j).ok());
            DialogTurnView { role, content, book_ref }
        })
        .collect();

    let q_state_str = serde_json::to_string(&session.q_state).unwrap().trim_matches('"').to_string();

    Ok(Json(SessionView {
        session_id: session.id,
        mode: session.mode,
        index: session.cursor + 1,
        total: session.queue.len(),
        q_state: q_state_str,
        question: q_view,
        empty: Some(false),
        message: None,
        dialog,
    }))
}

async fn next_question(
    State(state): State<TutorState>,
    Path(id): Path<String>,
) -> Result<Json<SessionView>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let mut session = load_session(&mut *conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    if !session.can_next() {
        return Err(ApiError::Conflict("Question not completed yet".into()));
    }

    if !session.advance() {
        return Err(ApiError::Conflict("Already reached end of queue".into()));
    }

    save_session(&mut *conn, &session)
        .await
        .map_err(ApiError::from)?;

    drop(conn);
    get_session(State(state), Path(id)).await
}

async fn end_session(
    State(state): State<TutorState>,
    Path(id): Path<String>,
) -> Result<Json<EndSessionReport>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let mut session = load_session(&mut *conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    session.finish(Utc::now());
    save_session(&mut *conn, &session)
        .await
        .map_err(ApiError::from)?;

    // Stats
    let answered_row: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT question_id) FROM attempts WHERE session_id = ?"
    )
    .bind(&id)
    .fetch_one(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    let first_try_row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM attempts WHERE session_id = ? AND try_no = 1 AND verdict = 'correct'"
    )
    .bind(&id)
    .fetch_one(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    let weak_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT concept_tag FROM concept_weakness WHERE student_id = ? AND score >= 1.0 ORDER BY score DESC LIMIT 5"
    )
    .bind(session.student_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    Ok(Json(EndSessionReport {
        answered: answered_row.0,
        first_try_correct: first_try_row.0,
        weak_concepts: weak_rows.into_iter().map(|(t,)| t).collect(),
    }))
}

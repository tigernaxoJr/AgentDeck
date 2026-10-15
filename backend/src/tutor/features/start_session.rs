use axum::{
    extract::State,
    routing::post,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::tutor::domain::session::{select_queue, Candidates, QState, Session};
use crate::tutor::shared::dto::{DialogTurnView, QuestionView, SessionView};
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{load_question, save_session};
use crate::tutor::TutorState;

#[derive(Debug, Deserialize)]
pub struct StartSessionReq {
    pub student_id: i64,
    pub mode: String, // 'progress' | 'review' | 'mixed'
    pub subject_id: String,
    pub unit_id: Option<String>,
    pub limit: Option<usize>,
}

pub fn routes() -> Router<TutorState> {
    Router::new().route("/sessions", post(start_session))
}

async fn start_session(
    State(state): State<TutorState>,
    Json(payload): Json<StartSessionReq>,
) -> Result<Json<SessionView>, ApiError> {
    let limit = payload.limit.unwrap_or(15);
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    // 1. Gather candidates
    let progress_ids: Vec<String> = if let Some(ref uid) = payload.unit_id {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT q.id
            FROM questions q
            LEFT JOIN mastery m ON m.question_id = q.id AND m.student_id = ?
            WHERE q.unit_id = ? AND q.active = 1 AND (m.box IS NULL OR m.box < 3)
            ORDER BY q.ord ASC
            "#,
        )
        .bind(payload.student_id)
        .bind(uid)
        .fetch_all(&mut *conn)
        .await
        .map_err(ApiError::from)?;
        rows.into_iter().map(|(id,)| id).collect()
    } else {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT q.id
            FROM questions q
            LEFT JOIN mastery m ON m.question_id = q.id AND m.student_id = ?
            WHERE q.subject_id = ? AND q.active = 1 AND (m.box IS NULL OR m.box < 3)
            ORDER BY q.ord ASC
            "#,
        )
        .bind(payload.student_id)
        .bind(&payload.subject_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(ApiError::from)?;
        rows.into_iter().map(|(id,)| id).collect()
    };

    let due_ids: Vec<String> = {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT q.id
            FROM mastery m
            JOIN questions q ON q.id = m.question_id
            WHERE m.student_id = ? AND q.subject_id = ? AND q.active = 1 AND m.due_at <= ?
            ORDER BY m.box ASC, m.due_at ASC
            "#,
        )
        .bind(payload.student_id)
        .bind(&payload.subject_id)
        .bind(&now_str)
        .fetch_all(&mut *conn)
        .await
        .map_err(ApiError::from)?;
        rows.into_iter().map(|(id,)| id).collect()
    };

    let weak_concept_ids: Vec<String> = {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT q.id
            FROM questions q
            JOIN concept_weakness cw ON cw.student_id = ? AND cw.score >= 1.0
            WHERE q.subject_id = ? AND q.active = 1 AND instr(q.concept_tags_json, cw.concept_tag) > 0
            ORDER BY cw.score DESC
            "#,
        )
        .bind(payload.student_id)
        .bind(&payload.subject_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(ApiError::from)?;
        rows.into_iter().map(|(id,)| id).collect()
    };

    let wrong_ids: Vec<String> = {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT q.id
            FROM mastery m
            JOIN questions q ON q.id = m.question_id
            WHERE m.student_id = ? AND q.subject_id = ? AND q.active = 1 AND m.total_wrong > 0
            ORDER BY m.total_wrong DESC
            "#,
        )
        .bind(payload.student_id)
        .bind(&payload.subject_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(ApiError::from)?;
        rows.into_iter().map(|(id,)| id).collect()
    };

    let candidates = Candidates {
        progress_ids,
        due_ids,
        weak_concept_ids,
        wrong_ids,
    };

    let seed = (now.timestamp_millis() as u64) ^ (payload.student_id as u64);
    let queue = select_queue(&payload.mode, &candidates, limit, seed);

    if queue.is_empty() {
        return Ok(Json(SessionView {
            session_id: String::new(),
            mode: payload.mode,
            index: 0,
            total: 0,
            q_state: "asking".to_string(),
            question: None,
            empty: Some(true),
            message: Some("目前沒有需要複習的題目 🎉".to_string()),
            dialog: vec![],
        }));
    }

    let session_id = Uuid::new_v4().to_string();
    let session = Session {
        id: session_id.clone(),
        student_id: payload.student_id,
        mode: payload.mode.clone(),
        subject_id: payload.subject_id.clone(),
        unit_id: payload.unit_id.clone(),
        queue: queue.clone(),
        cursor: 0,
        q_state: QState::Asking,
        q_try: 0,
        q_hinted: false,
        started_at: now,
        ended_at: None,
    };

    save_session(&mut *conn, &session)
        .await
        .map_err(ApiError::from)?;

    let first_qid = &queue[0];
    let q = load_question(&mut *conn, first_qid)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;

    let q_view = QuestionView::from(&q);

    Ok(Json(SessionView {
        session_id,
        mode: payload.mode,
        index: 1,
        total: queue.len(),
        q_state: "asking".to_string(),
        question: Some(q_view),
        empty: Some(false),
        message: None,
        dialog: vec![],
    }))
}

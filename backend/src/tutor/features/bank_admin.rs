use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;

use crate::tutor::domain::question::Question;
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{load_question, save_question};
use crate::tutor::TutorState;

#[derive(Debug, Deserialize)]
pub struct UpdateQuestionReq {
    pub book_page: Option<i64>,
    pub book_where: Option<String>,
    pub concept_tags: Option<Vec<String>>,
    pub active: Option<bool>,
}

pub fn routes() -> Router<TutorState> {
    Router::new()
        .route("/admin/questions/{id}", get(get_admin_question).patch(update_admin_question))
}

async fn get_admin_question(
    State(state): State<TutorState>,
    Path(id): Path<String>,
) -> Result<Json<Question>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let q = load_question(&mut conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;

    Ok(Json(q))
}

async fn update_admin_question(
    State(state): State<TutorState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateQuestionReq>,
) -> Result<Json<Question>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let mut q = load_question(&mut conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;

    if let Some(bp) = payload.book_page {
        q.book_page = Some(bp);
    }
    if let Some(bw) = payload.book_where {
        q.book_where = Some(bw);
    }
    if let Some(ct) = payload.concept_tags {
        q.concept_tags = ct;
    }
    if let Some(act) = payload.active {
        q.active = act;
    }
    q.updated_at = Utc::now().to_rfc3339();

    save_question(&mut conn, &q).await.map_err(ApiError::from)?;

    Ok(Json(q))
}

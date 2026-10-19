use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::Serialize;

use crate::tutor::shared::dto::QuestionView;
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::load_question;
use crate::tutor::TutorState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeakConceptReportItem {
    pub tag: String,
    pub score: f64,
    pub wrong_count: i64,
    pub last_note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptReportItem {
    pub verdict: String,
    pub input_image_url: Option<String>,
    pub input_text: Option<String>,
    pub recognized_text: Option<String>,
    pub graded_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WrongQuestionReportItem {
    pub question: QuestionView,
    pub attempts: Vec<AttemptReportItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudentReportView {
    pub weak_concepts: Vec<WeakConceptReportItem>,
    pub due_count: i64,
    pub recent_wrong: Vec<WrongQuestionReportItem>,
}

/// verdict, input_image_path, input_text, recognized_text, graded_by, created_at
type AttemptRow = (String, Option<String>, Option<String>, Option<String>, String, String);

pub fn routes() -> Router<TutorState> {
    Router::new().route("/students/{id}/report", get(get_student_report))
}

async fn get_student_report(
    State(state): State<TutorState>,
    Path(student_id): Path<i64>,
) -> Result<Json<StudentReportView>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let now_str = Utc::now().to_rfc3339();

    // 1. Weak concepts
    let weak_rows: Vec<(String, f64, i64, Option<String>)> = sqlx::query_as(
        "SELECT concept_tag, score, wrong_count, last_note FROM concept_weakness WHERE student_id = ? AND score > 0 ORDER BY score DESC LIMIT 10"
    )
    .bind(student_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    let weak_concepts = weak_rows
        .into_iter()
        .map(|(tag, score, wrong_count, last_note)| WeakConceptReportItem {
            tag,
            score,
            wrong_count,
            last_note,
        })
        .collect();

    // 2. Due count
    let due_row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM mastery WHERE student_id = ? AND due_at <= ?"
    )
    .bind(student_id)
    .bind(&now_str)
    .fetch_one(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    // 3. Recent wrong questions
    let recent_q_rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT question_id
        FROM attempts
        WHERE student_id = ? AND verdict IN ('incorrect', 'partial')
        ORDER BY created_at DESC
        LIMIT 10
        "#
    )
    .bind(student_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    let mut recent_wrong = Vec::new();
    for (qid,) in recent_q_rows {
        if let Some(q) = load_question(&mut conn, &qid).await.map_err(ApiError::from)? {
            let att_rows: Vec<AttemptRow> = sqlx::query_as(
                r#"
                SELECT verdict, input_image_path, input_text, recognized_text, graded_by, created_at
                FROM attempts
                WHERE student_id = ? AND question_id = ?
                ORDER BY try_no ASC
                "#
            )
            .bind(student_id)
            .bind(&qid)
            .fetch_all(&mut *conn)
            .await
            .map_err(ApiError::from)?;

            let attempts = att_rows
                .into_iter()
                .map(|(verdict, img, text, rec, graded_by, created_at)| AttemptReportItem {
                    verdict,
                    input_image_url: img.map(|p| format!("/api/tutor/assets/{p}")),
                    input_text: text,
                    recognized_text: rec,
                    graded_by,
                    created_at,
                })
                .collect();

            recent_wrong.push(WrongQuestionReportItem {
                question: QuestionView::from(&q),
                attempts,
            });
        }
    }

    Ok(Json(StudentReportView {
        weak_concepts,
        due_count: due_row.0,
        recent_wrong,
    }))
}

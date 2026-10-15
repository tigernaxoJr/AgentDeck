use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::tutor::shared::error::ApiError;
use crate::tutor::TutorState;

#[derive(Debug, Serialize, Deserialize)]
pub struct StudentItem {
    pub id: i64,
    pub name: String,
    pub grade: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateStudentReq {
    pub name: String,
    #[serde(default = "default_grade")]
    pub grade: i64,
}

fn default_grade() -> i64 {
    4
}

pub fn routes() -> Router<TutorState> {
    Router::new()
        .route("/students", get(list_students).post(create_student))
}

async fn list_students(
    State(state): State<TutorState>,
) -> Result<Json<Vec<StudentItem>>, ApiError> {
    let rows: Vec<(i64, String, i64)> = sqlx::query_as("SELECT id, name, grade FROM students ORDER BY id ASC")
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::from)?;

    let items = rows
        .into_iter()
        .map(|(id, name, grade)| StudentItem { id, name, grade })
        .collect();

    Ok(Json(items))
}

async fn create_student(
    State(state): State<TutorState>,
    Json(payload): Json<CreateStudentReq>,
) -> Result<Json<StudentItem>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(ApiError::BadRequest("Student name cannot be empty".into()));
    }

    let now = Utc::now().to_rfc3339();
    let res = sqlx::query("INSERT INTO students (name, grade, created_at) VALUES (?, ?, ?)")
        .bind(payload.name.trim())
        .bind(payload.grade)
        .bind(&now)
        .execute(&state.pool)
        .await
        .map_err(ApiError::from)?;

    let id = res.last_insert_rowid();

    Ok(Json(StudentItem {
        id,
        name: payload.name.trim().to_string(),
        grade: payload.grade,
    }))
}

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::BadRequest(m) => write!(f, "BadRequest: {m}"),
            ApiError::NotFound(m) => write!(f, "NotFound: {m}"),
            ApiError::Conflict(m) => write!(f, "Conflict: {m}"),
            ApiError::Internal(m) => write!(f, "Internal: {m}"),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, msg) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", m),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, "NOT_FOUND", m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "CONFLICT", m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", m),
        };

        (status, Json(ErrorBody { error: code, message: msg })).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::Internal(format!("Database error: {e}"))
    }
}

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue},
    response::Response,
    routing::get,
    Router,
};

use crate::tutor::shared::error::ApiError;
use crate::tutor::TutorState;

pub fn routes() -> Router<TutorState> {
    Router::new().route("/assets/{*path}", get(serve_asset))
}

async fn serve_asset(
    State(state): State<TutorState>,
    Path(path): Path<String>,
) -> Result<Response, ApiError> {
    let clean_path = path.trim_start_matches('/').trim_start_matches('\\');

    let base_assets = state.data_dir.join("assets");
    let base_answers = state.data_dir.join("answers");

    let candidate1 = base_assets.join(clean_path);
    let candidate2 = base_answers.join(clean_path);

    let target = if candidate1.exists() {
        candidate1
    } else if candidate2.exists() {
        candidate2
    } else {
        return Err(ApiError::NotFound(format!("Asset not found: {clean_path}")));
    };

    let canonical_target = target
        .canonicalize()
        .map_err(|_| ApiError::NotFound("Invalid path".into()))?;

    let is_safe = [&base_assets, &base_answers].into_iter().any(|base| {
        base.canonicalize()
            .is_ok_and(|base| canonical_target.starts_with(base))
    });

    if !is_safe {
        return Err(ApiError::BadRequest("Forbidden path traversal".into()));
    }

    let bytes = tokio::fs::read(&canonical_target)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read asset: {e}")))?;

    let mime = if canonical_target.extension().is_some_and(|ext| ext == "png") {
        "image/png"
    } else if canonical_target.extension().is_some_and(|ext| ext == "jpg" || ext == "jpeg") {
        "image/jpeg"
    } else {
        "application/octet-stream"
    };

    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(mime),
    );
    Ok(response)
}

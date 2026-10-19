use axum::{
    extract::{Query, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;

use std::sync::Arc;

#[derive(Deserialize)]
pub struct WsAuthQuery {
    pub token: Option<String>,
}

/// Axum 驗證中介軟體：同時支援 Bearer Header (REST) 與 ?token= Query (WebSocket)
/// State 為 API Key 本身，讓 Console 與 Tutor 的 router 都能共用此中介軟體。
pub async fn require_api_key(
    State(api_key): State<Arc<String>>,
    Query(query): Query<WsAuthQuery>,
    req: Request,
    next: Next,
) -> Response {
    // 若環境變數未設定 API Key (空字串)，則視為開發模式直接放行
    if api_key.is_empty() {
        return next.run(req).await;
    }

    // 1. 優先嘗試從 Authorization: Bearer <token> 提取
    let header_token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|val| val.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim);

    // 2. 次要嘗試從 URL Query (?token=<token>) 提取 (專供 WebSocket 握手使用)
    let provided_token = header_token.or(query.token.as_deref());

    match provided_token {
        Some(token) if constant_time_eq(token.as_bytes(), api_key.as_bytes()) => {
            next.run(req).await
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "UNAUTHORIZED",
                "message": "無效或缺失的 API Key，請於前端設定正確的存取金鑰。"
            })),
        )
            .into_response(),
    }
}

/// 固定時間比對，防止 Timing Attack
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

use std::time::Duration;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use ai_cli::{AiPurpose, Capabilities, RunFile};
use crate::tutor::ai::prompts::PromptContext;
use crate::tutor::ai::turn::call_turn;
use crate::tutor::domain::question::{Answer, InputMode, QType, Question};
use crate::tutor::domain::session::QState;
use crate::tutor::shared::error::ApiError;
use crate::tutor::TutorState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendItemView {
    pub id: String,
    pub version: Option<String>,
    pub healthy: bool,
    pub capabilities: Capabilities,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendsAdminView {
    pub available: Vec<BackendItemView>,
    pub active: Vec<String>,
    pub active_vision: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBackendsReq {
    pub active: Vec<String>,
    pub active_vision: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestBackendReq {
    pub backend_id: String,
    #[serde(default)]
    pub with_image: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestBackendReply {
    pub ok: bool,
    pub duration_ms: u128,
    pub reply: Option<String>,
    pub error: Option<String>,
}

pub fn routes() -> Router<TutorState> {
    Router::new()
        .route("/admin/ai/backends", get(get_backends).put(update_backends))
        .route("/admin/ai/test", post(test_backend))
        .route("/admin/ai/calls", get(get_recent_ai_calls))
}

async fn get_backends(
    State(state): State<TutorState>,
) -> Result<Json<BackendsAdminView>, ApiError> {
    let main_engines = state.ai.engines.read().await.clone();
    let vision_engines = state.ai.vision_engines.read().await.clone();

    let active = main_engines.iter().map(|e| e.id().to_string()).collect();
    let active_vision = vision_engines.iter().map(|e| e.id().to_string()).collect();

    // Check all known IDs
    let all_candidates = ["agy", "claude", "gemini", "codex"];
    let mut available = Vec::new();

    for id in all_candidates {
        let temp_eng: Vec<std::sync::Arc<dyn ai_cli::AiEngine>> = ai_cli::Registry::build(&ai_cli::BackendConfig {
            active_ids: vec![id.to_string()],
            program_overrides: std::collections::HashMap::new(),
            model_overrides: std::collections::HashMap::new(),
            toml_path: Some(state.data_dir.join("ai_backends.toml")),
            runs_dir: state.data_dir.join("runs"),
        });

        if let Some(eng) = temp_eng.first() {
            let (healthy, ver) = match eng.health_check().await {
                Ok(v) => (true, Some(v)),
                Err(e) => (false, Some(e.to_string())),
            };
            available.push(BackendItemView {
                id: id.to_string(),
                version: ver,
                healthy,
                capabilities: eng.capabilities().clone(),
                model: eng.model(),
            });
        }
    }

    Ok(Json(BackendsAdminView {
        available,
        active,
        active_vision,
    }))
}

async fn update_backends(
    State(state): State<TutorState>,
    Json(payload): Json<UpdateBackendsReq>,
) -> Result<Json<BackendsAdminView>, ApiError> {
    // Validate known IDs
    for id in &payload.active {
        if !["agy", "claude", "gemini", "codex"].contains(&id.as_str()) {
            return Err(ApiError::BadRequest(format!("Unknown backend id: {id}")));
        }
    }

    let active_str = payload.active.join(",");
    let vision_str = payload.active_vision.as_ref().map(|v| v.join(",")).unwrap_or_else(|| active_str.clone());

    state.ai.set_setting("ai_backends", &active_str).await.map_err(ApiError::from)?;
    state.ai.set_setting("ai_backends_vision", &vision_str).await.map_err(ApiError::from)?;

    get_backends(State(state)).await
}

async fn test_backend(
    State(state): State<TutorState>,
    Json(payload): Json<TestBackendReq>,
) -> Result<Json<TestBackendReply>, ApiError> {
    let engs = ai_cli::Registry::build(&ai_cli::BackendConfig {
        active_ids: vec![payload.backend_id.clone()],
        program_overrides: std::collections::HashMap::new(),
        model_overrides: std::collections::HashMap::new(),
        toml_path: Some(state.data_dir.join("ai_backends.toml")),
        runs_dir: state.data_dir.join("runs"),
    });

    let eng = match engs.first() {
        Some(e) => e.clone(),
        None => return Err(ApiError::BadRequest(format!("Backend '{}' not found", payload.backend_id))),
    };

    let test_q = Question {
        id: "test_q".into(),
        subject_id: "social".into(),
        unit_id: "u1".into(),
        ord: 1,
        section: "".into(),
        qtype: QType::SingleChoice,
        stem: "1 + 1 等於多少？".into(),
        options: vec!["1".into(), "2".into()],
        answer: Answer::Choice(1),
        explanation: "1加1等於2".into(),
        keywords: vec![],
        char_count: None,
        image_path: None,
        book_page: Some(1),
        book_where: Some("中間".into()),
        concept_tags: vec![],
        input_mode: InputMode::Choice,
        source: "test".into(),
        active: true,
        updated_at: "".into(),
    };

    let ctx = PromptContext {
        student_name: "測試生".into(),
        student_grade: 4,
        subject_name: "數學".into(),
        book_title: "測試書".into(),
        question: test_q,
        try_no: 1,
        dialog: vec![],
        weak_concepts: vec![],
        student_choice: Some("1".into()),
        student_message: None,
        q_state: QState::Asking,
    };

    let start = std::time::Instant::now();
    let run_files = vec![
        RunFile {
            name: "context.json".into(),
            bytes: crate::tutor::ai::prompts::build_context_json(&ctx).into_bytes(),
        },
    ];

    let engines = vec![eng];
    let res = call_turn(
        &engines,
        AiPurpose::TutorWrong,
        &ctx,
        run_files,
        vec![],
        &state.pool,
        None,
        Duration::from_secs(60),
    )
    .await;

    let duration_ms = start.elapsed().as_millis();
    match res {
        Ok(fb) => Ok(Json(TestBackendReply {
            ok: true,
            duration_ms,
            reply: Some(fb.say),
            error: None,
        })),
        Err(e) => Ok(Json(TestBackendReply {
            ok: false,
            duration_ms,
            reply: None,
            error: Some(e.to_string()),
        })),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCallItemView {
    pub id: String,
    pub backend: String,
    pub model: Option<String>,
    pub purpose: String,
    pub session_id: Option<String>,
    pub question_id: Option<String>,
    pub duration_ms: Option<i64>,
    pub ok: bool,
    pub error: Option<String>,
    pub raw_output: Option<String>,
    pub prompt: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub created_at: String,
}

/// ai_calls 的一列，欄位順序同下方 SELECT
type AiCallRow = (
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    String,
);

async fn get_recent_ai_calls(
    State(state): State<TutorState>,
) -> Result<Json<Vec<AiCallItemView>>, ApiError> {
    let rows: Vec<AiCallRow> = sqlx::query_as(
        r#"
        SELECT id, backend, model, purpose, session_id, question_id,
               duration_ms, ok, error, raw_output, prompt, input_tokens, output_tokens, created_at
        FROM ai_calls
        ORDER BY created_at DESC
        LIMIT 20
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let views = rows
        .into_iter()
        .map(|r| AiCallItemView {
            id: r.0,
            backend: r.1,
            model: r.2,
            purpose: r.3,
            session_id: r.4,
            question_id: r.5,
            duration_ms: r.6,
            ok: r.7 != 0,
            error: r.8,
            raw_output: r.9,
            prompt: r.10,
            input_tokens: r.11,
            output_tokens: r.12,
            created_at: r.13,
        })
        .collect();

    Ok(Json(views))
}

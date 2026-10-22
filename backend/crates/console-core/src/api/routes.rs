use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use tokio::process::Command;
use axum::http::HeaderValue;
use tower_http::cors::AllowOrigin;
use uuid::Uuid;

use crate::api::auth::require_api_key;
use crate::api::ws::ws_handler;
use crate::executor::scheduler::TaskScheduler;
use crate::models::protocol::{
    CommitChangesRequest, CreateTaskRequest, GitDiffReport, RevertFileRequest, TaskMetadata,
};
use crate::storage::db::TaskRepository;

#[derive(Clone)]
pub struct AppState {
    pub scheduler: Arc<TaskScheduler>,
    pub repo: TaskRepository,
    pub api_key: Arc<String>,
}

impl AppState {
    pub fn new(scheduler: TaskScheduler, repo: TaskRepository, api_key: String) -> Self {
        Self {
            scheduler: Arc::new(scheduler),
            repo,
            api_key: Arc::new(api_key),
        }
    }
}

/// 建立 Console 專屬的 protected API Router
pub fn create_console_router(state: AppState) -> Router {
    Router::new()
        .route("/tasks", post(create_task_handler).get(list_tasks_handler))
        .route("/tasks/{id}", get(get_task_handler))
        .route("/tasks/{id}/cancel", delete(cancel_task_handler))
        .route("/tasks/{id}/diff", get(get_task_diff_handler))
        .route("/tasks/{id}/diff/revert", post(revert_file_handler))
        .route("/tasks/{id}/diff/commit", post(commit_changes_handler))
        .route("/models", get(list_models_handler))
        .route("/workspaces", get(list_workspaces_handler))
        .route("/ws", get(ws_handler))
        .route_layer(middleware::from_fn_with_state(
            state.api_key.clone(),
            require_api_key,
        ))
        .with_state(state)
}

/// 跨域白名單：前端預設同源存取，只有直連後端的開發情境才需要。
/// `CORS_ORIGINS` 以逗號分隔，預設為 Vite dev server 的兩個位址。
pub fn cors_origins() -> AllowOrigin {
    let raw = std::env::var("CORS_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:5173,http://127.0.0.1:5173".to_string());
    let origins: Vec<HeaderValue> = raw
        .split(',')
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    AllowOrigin::list(origins)
}

/// POST /api/tasks - 建立新任務
async fn create_task_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskMetadata>), (StatusCode, String)> {
    if req.prompt.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Prompt 內容不得為空".to_string()));
    }

    let metadata = state.scheduler.enqueue(req);
    state
        .repo
        .insert_task(&metadata)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((StatusCode::CREATED, Json(metadata)))
}

/// GET /api/tasks - 取得最近任務清單
async fn list_tasks_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<TaskMetadata>>, (StatusCode, String)> {
    let list = state
        .repo
        .list_recent_tasks(100)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(list))
}

/// GET /api/tasks/{id} - 查詢單一任務
async fn get_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<TaskMetadata>, (StatusCode, String)> {
    let task = state
        .repo
        .get_task_by_id(task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "找不到指定的任務".to_string()))?;

    Ok(Json(task))
}

/// DELETE /api/tasks/{id}/cancel - 中止任務
async fn cancel_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> impl IntoResponse {
    if state.scheduler.abort_task(task_id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// GET /api/tasks/{id}/diff - 取得任務 Git Diff 報告
async fn get_task_diff_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<GitDiffReport>, (StatusCode, String)> {
    let target_dir = resolve_task_workspace(&state, task_id).await?;
    let report = crate::executor::git::GitInspector::capture_workspace_diff(&target_dir).await;
    Ok(Json(report))
}

/// POST /api/tasks/{id}/diff/revert - 還原單一修改或刪除新檔案
async fn revert_file_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(payload): Json<RevertFileRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let target_dir = resolve_task_workspace(&state, task_id).await?;

    let is_untracked = payload.file_path.starts_with("??")
        || is_file_untracked(&target_dir, &payload.file_path).await;

    if is_untracked {
        let clean_path = payload
            .file_path
            .trim_start_matches('?')
            .trim();
        let target_file = target_dir.join(clean_path);
        tokio::fs::remove_file(target_file)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(StatusCode::OK)
    } else {
        let out = Command::new("git")
            .current_dir(&target_dir)
            .args(["checkout", "HEAD", "--", &payload.file_path])
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if out.status.success() {
            Ok(StatusCode::OK)
        } else {
            let err_msg = String::from_utf8_lossy(&out.stderr).to_string();
            Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg))
        }
    }
}

/// POST /api/tasks/{id}/diff/commit - 安全 Commit
async fn commit_changes_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(payload): Json<CommitChangesRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let target_dir = resolve_task_workspace(&state, task_id).await?;

    // 取得任務 Prompt 作為預設 Commit 訊息
    let task = state
        .repo
        .get_task_by_id(task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "找不到指定任務".to_string()))?;

    let commit_msg = payload.message.unwrap_or_else(|| {
        let first_line = task
            .prompt
            .lines()
            .next()
            .unwrap_or("chore: apply agent changes")
            .trim();
        format!("feat(agent): {}", first_line)
    });

    // 1. git add . 排除 .antigravity_out
    let add_out = Command::new("git")
        .current_dir(&target_dir)
        .args(["add", "--", ".", ":(exclude).antigravity_out"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !add_out.status.success() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            String::from_utf8_lossy(&add_out.stderr).to_string(),
        ));
    }

    // 2. git commit -m
    let commit_out = Command::new("git")
        .current_dir(&target_dir)
        .args(["commit", "-m", &commit_msg])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !commit_out.status.success() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            String::from_utf8_lossy(&commit_out.stderr).to_string(),
        ));
    }

    Ok(StatusCode::OK)
}

async fn resolve_task_workspace(
    state: &AppState,
    task_id: Uuid,
) -> Result<PathBuf, (StatusCode, String)> {
    let task = state
        .repo
        .get_task_by_id(task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "找不到指定的任務".to_string()))?;

    let base = std::fs::canonicalize(state.scheduler.base_workspace())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let target_raw = if task.workspace_path.trim().is_empty() || task.workspace_path == "." {
        base.clone()
    } else {
        base.join(&task.workspace_path)
    };

    let target = std::fs::canonicalize(target_raw)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("工作目錄無效: {}", e)))?;

    if !target.starts_with(&base) {
        return Err((StatusCode::FORBIDDEN, "越權路徑存取".to_string()));
    }

    Ok(target)
}

async fn is_file_untracked(work_dir: &std::path::Path, file_path: &str) -> bool {
    let out = Command::new("git")
        .current_dir(work_dir)
        .args(["status", "--porcelain", "--", file_path])
        .output()
        .await;

    if let Ok(res) = out {
        let text = String::from_utf8_lossy(&res.stdout);
        text.starts_with("??")
    } else {
        false
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// GET /api/models - 取得 CLI 支援的模型清單
async fn list_models_handler() -> Result<Json<Vec<ModelInfo>>, (StatusCode, String)> {
    let bin_name = std::env::var("ANTIGRAVITY_CLI_BIN").unwrap_or_else(|_| "agy".to_string());
    let output = Command::new(&bin_name)
        .arg("models")
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("執行 {} models 失敗: {}", bin_name, e)))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    for line in stdout_str.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Fetching") || trimmed.starts_with("Available") {
            continue;
        }
        // 解析格式例如 "gemini-3.8-flash-high\tGemini 3.8 Flash (High)" 或空白分隔
        let parts: Vec<&str> = trimmed.splitn(2, ['\t', ' ']).collect();
        if !parts.is_empty() {
            let id = parts[0].trim().to_string();
            let name = if parts.len() > 1 && !parts[1].trim().is_empty() {
                parts[1].trim().to_string()
            } else {
                id.clone()
            };
            if !id.is_empty() {
                models.push(ModelInfo { id, name });
            }
        }
    }

    Ok(Json(models))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub path: String,
    pub name: String,
}

/// GET /api/workspaces - 取得目前主專案與常見工作目錄清單供下拉選取
async fn list_workspaces_handler() -> Result<Json<Vec<WorkspaceInfo>>, (StatusCode, String)> {
    let mut workspaces = Vec::new();

    // 1. 當前專案根目錄
    if let Ok(current_dir) = std::env::current_dir() {
        let path_str = current_dir.to_string_lossy().to_string();
        let name = current_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Current Directory".to_string());
        workspaces.push(WorkspaceInfo {
            path: path_str,
            name: format!("[專案根目錄] {}", name),
        });

        // 2. 當前專案的主要子模組目錄
        for sub in &["backend", "frontend"] {
            let sub_path = current_dir.join(sub);
            if sub_path.is_dir() {
                workspaces.push(WorkspaceInfo {
                    path: sub_path.to_string_lossy().to_string(),
                    name: format!("[子目錄] {}", sub),
                });
            }
        }

        // 3. 掃描同層或上一層 workspace 目錄（如果有）
        if let Some(parent) = current_dir.parent()
            && let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path != current_dir
                        && let Some(file_name) = path.file_name() {
                            let name_str = file_name.to_string_lossy().to_string();
                            if !name_str.starts_with('.') {
                                workspaces.push(WorkspaceInfo {
                                    path: path.to_string_lossy().to_string(),
                                    name: name_str,
                                });
                            }
                        }
                }
            }
    }

    Ok(Json(workspaces))
}

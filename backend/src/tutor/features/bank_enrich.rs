use std::time::Duration;
use axum::{
    extract::State,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use ai_cli::{call_json, AiEngine, AiPurpose, CallSpec, RunFile};
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{load_question, save_question};
use crate::tutor::TutorState;

#[derive(Debug, Deserialize)]
pub struct EnrichReq {
    pub subject_id: String,
}

#[derive(Debug, Serialize)]
pub struct EnrichReply {
    pub job_started: bool,
}

#[derive(Debug, Deserialize)]
pub struct EnrichItem {
    pub id: String,
    pub where_clause: Option<String>,
    #[serde(rename = "where")]
    pub where_alias: Option<String>,
    #[serde(default)]
    pub concept_tags: Vec<String>,
}

pub fn routes() -> Router<TutorState> {
    Router::new().route("/admin/enrich", post(handle_enrich))
}

async fn handle_enrich(
    State(state): State<TutorState>,
    Json(payload): Json<EnrichReq>,
) -> Result<Json<EnrichReply>, ApiError> {
    let state_clone = state.clone();
    let sub_id = payload.subject_id;

    // Spawn background enrichment job
    tokio::spawn(async move {
        let scans_dir = state_clone.data_dir.join("assets").join(&sub_id).join("scans");
        if !scans_dir.exists() {
            return;
        }

        let engines = state_clone.ai.engines_for(AiPurpose::EnrichPage).await;
        if engines.is_empty() {
            return;
        }

        // Get questions for this subject that need enrich (book_page or book_where is null/empty)
        let mut conn = match state_clone.pool.acquire().await {
            Ok(c) => c,
            Err(_) => return,
        };

        let q_rows: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT id, stem
            FROM questions
            WHERE subject_id = ? AND (book_page IS NULL OR book_where IS NULL OR book_where = '')
            "#
        )
        .bind(&sub_id)
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_default();

        if q_rows.is_empty() {
            return;
        }

        let offset_row: (i64,) = sqlx::query_as("SELECT page_offset FROM subjects WHERE id = ?")
            .bind(&sub_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap_or((0,));
        let page_offset = offset_row.0;

        if let Ok(mut entries) = tokio::fs::read_dir(&scans_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                if file_name.contains("解答") || !file_name.ends_with(".jpg") {
                    continue;
                }

                // Deduce scan number e.g. 095.jpg or 社會-003.jpg
                let scan_num = file_name
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<i64>()
                    .ok();

                let deduced_page = scan_num.map(|n| n + page_offset);

                let page_bytes = match tokio::fs::read(&path).await {
                    Ok(b) => b,
                    Err(_) => continue,
                };

                let prompt_text = format!(
                    r#"任務：找出哪些題目印在這一頁，以及在頁面的哪個位置（例如『左上第二大題』『右下角的地圖』），輸出 JSON 陣列：
[
  {{"id":"...","where":"...","concept_tags":["..."]}}
]
候選題目列表：
{}"#,
                    serde_json::to_string(&q_rows).unwrap_or_default()
                );

                let prompt_builder = move |_caps: &ai_cli::Capabilities| prompt_text.clone();

                let spec = CallSpec {
                    purpose: AiPurpose::EnrichPage,
                    needs_vision: true,
                    build_prompt: &prompt_builder,
                    files: vec![RunFile {
                        name: "page.jpg".into(),
                        bytes: page_bytes,
                    }],
                    images: vec!["page.jpg".into()],
                    output_file: "out/enrich.json",
                    timeout: Duration::from_secs(120),
                };

                let (res, _) = call_json::<Vec<EnrichItem>>(&engines, spec).await;
                if let Ok(items) = res {
                    for item in items {
                        let where_clause = item.where_clause.or(item.where_alias).unwrap_or_default();
                        if let Ok(Some(mut q)) = load_question(&mut *conn, &item.id).await {
                            let mut changed = false;
                            if q.book_page.is_none() {
                                q.book_page = deduced_page;
                                changed = true;
                            }
                            if q.book_where.is_none() || q.book_where.as_deref() == Some("") {
                                q.book_where = Some(where_clause);
                                changed = true;
                            }
                            for tag in item.concept_tags {
                                if !q.concept_tags.contains(&tag) {
                                    q.concept_tags.push(tag);
                                    changed = true;
                                }
                            }
                            if changed {
                                let _ = save_question(&mut *conn, &q).await;
                            }
                        }
                    }
                }
            }
        }
    });

    Ok(Json(EnrichReply { job_started: true }))
}

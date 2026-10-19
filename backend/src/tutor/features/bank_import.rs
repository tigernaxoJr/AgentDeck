use std::path::Path;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::tutor::domain::question::{normalize_import, RawImportQuestion};
use crate::tutor::store::save_question;

use axum::{
    extract::State,
    routing::post,
    Json, Router,
};
use crate::tutor::shared::error::ApiError;
use crate::tutor::TutorState;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImportStats {
    pub subjects: usize,
    pub units: usize,
    pub questions: usize,
}

#[derive(Debug, Deserialize)]
pub struct ImportReq {
    pub root: Option<String>,
}

pub fn routes() -> Router<TutorState> {
    Router::new().route("/admin/import", post(handle_import))
}

async fn handle_import(
    State(state): State<TutorState>,
    Json(payload): Json<ImportReq>,
) -> Result<Json<ImportStats>, ApiError> {
    let root_str = payload.root.unwrap_or_else(|| {
        std::env::var("TUTOR_IMPORT_ROOT").unwrap_or_else(|_| "C:/workspace/jamie-exam".to_string())
    });

    let stats = import_all(&state.pool, Path::new(&root_str), &state.data_dir)
        .await
        .map_err(ApiError::BadRequest)?;

    Ok(Json(stats))
}

pub async fn import_all(
    pool: &SqlitePool,
    root: &Path,
    data_dir: &Path,
) -> Result<ImportStats, String> {
    let questions_file = root.join("web").join("public").join("data").join("questions.json");
    let mut raw_list: Vec<RawImportQuestion> = Vec::new();

    if questions_file.exists()
        && let Ok(content) = tokio::fs::read_to_string(&questions_file).await
            && let Ok(list) = serde_json::from_str::<Vec<RawImportQuestion>>(&content) {
                raw_list.extend(list);
            }

    // Also scan data_dir for questions_*.json
    if let Ok(mut entries) = tokio::fs::read_dir(data_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let p = entry.path();
            if p.is_file() && p.file_name().and_then(|f| f.to_str()).is_some_and(|s| s.starts_with("questions") && s.ends_with(".json"))
                && let Ok(content) = tokio::fs::read_to_string(&p).await
                    && let Ok(list) = serde_json::from_str::<Vec<RawImportQuestion>>(&content) {
                        raw_list.extend(list);
                    }
        }
    }

    if raw_list.is_empty() {
        return Err("No questions found in questions.json or tutor_data/questions*.json".into());
    }

    let now = Utc::now().to_rfc3339();
    let mut stats = ImportStats::default();

    // Default subjects
    let subjects = [
        ("social", "社會", "四上 社會 評量", 0i64),
        ("nature", "自然", "四上 自然 評量", 0i64),
        ("chinese", "國文", "四上 國文 評量", 0i64),
        ("math", "數學", "四上 數學 評量", 0i64),
    ];

    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    for (s_id, s_name, s_book, s_offset) in &subjects {
        sqlx::query(
            r#"
            INSERT INTO subjects (id, name, book_title, page_offset)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                book_title = CASE WHEN subjects.book_title = '' THEN excluded.book_title ELSE subjects.book_title END
            "#,
        )
        .bind(s_id)
        .bind(s_name)
        .bind(s_book)
        .bind(s_offset)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    }
    stats.subjects = subjects.len();

    // Copy scans if available
    let scans_src = root.join("data").join("scans");
    if scans_src.exists() {
        let scan_targets = [("社會", "social"), ("自然", "nature")];
        for (sub_name, sub_id) in &scan_targets {
            let src_dir = scans_src.join(sub_name);
            if src_dir.exists() {
                let target_dir = data_dir.join("assets").join(sub_id).join("scans");
                let _ = tokio::fs::create_dir_all(&target_dir).await;
                if let Ok(mut entries) = tokio::fs::read_dir(&src_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        if path.is_file()
                            && let Some(file_name) = path.file_name() {
                                let dest = target_dir.join(file_name);
                                let _ = tokio::fs::copy(&path, &dest).await;
                            }
                    }
                }
            }
        }
    }

    // Process questions
    let mut unit_orders = std::collections::HashMap::new();
    let mut prev_book_page = None;

    for (idx, raw) in raw_list.into_iter().enumerate() {
        let sub_id = match raw.subject.trim() {
            "社會" => "social",
            "自然" => "nature",
            "國文" => "chinese",
            "數學" => "math",
            other => other,
        };

        // Ensure unit
        let unit_ord = unit_orders.len() as i64 + 1;
        let ord_in_unit = unit_orders.entry(raw.unit.clone()).or_insert(unit_ord);

        sqlx::query(
            r#"
            INSERT INTO units (id, subject_id, title, ord)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(&raw.unit)
        .bind(sub_id)
        .bind(&raw.unit)
        .bind(*ord_in_unit)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

        // Copy crop image if present
        if let Some(ref img_rel) = raw.image {
            let src_img = root.join("web").join("public").join(img_rel);
            if src_img.exists() {
                let dest_img = data_dir.join("assets").join(sub_id).join(img_rel);
                if let Some(parent) = dest_img.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::copy(&src_img, &dest_img).await;
            }
        }

        let q = normalize_import(raw, idx as i64 + 1, prev_book_page, 0, &now)?;
        prev_book_page = q.book_page;

        save_question(&mut conn, &q).await.map_err(|e| e.to_string())?;
        stats.questions += 1;
    }

    let unit_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM units")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    stats.units = unit_count.0 as usize;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tutor::domain::question::{Answer, InputMode};
    use crate::tutor::shared::db::open_url;
    use crate::tutor::store::load_question;

    #[tokio::test]
    async fn test_import_all_on_real_data() {
        // 題庫素材有版權、不在 repo 內（CI 也沒有），找不到就跳過
        let root_str = std::env::var("TUTOR_IMPORT_ROOT")
            .unwrap_or_else(|_| "C:/workspace/jamie-exam".to_string());
        let root = Path::new(&root_str);
        if !root.exists() {
            eprintln!("skip: import root {root_str} not found");
            return;
        }
        let pool = open_url("sqlite::memory:").await.expect("open memory db");
        let data_dir = Path::new("target/test_tutor_data");

        let stats = import_all(&pool, root, data_dir).await.expect("import all");
        assert_eq!(stats.questions, 54);

        let mut conn = pool.acquire().await.unwrap();
        // check soc_01_mc_1
        let q_mc = load_question(&mut conn, "soc_01_mc_1")
            .await
            .unwrap()
            .expect("soc_01_mc_1 exists");
        assert_eq!(q_mc.answer, Answer::Choice(1));

        // check soc_01_map_1
        let q_map = load_question(&mut conn, "soc_01_map_1")
            .await
            .unwrap()
            .expect("soc_01_map_1 exists");
        assert_eq!(q_map.book_page, Some(3));
        assert_eq!(q_map.input_mode, InputMode::ImageCanvas);

        // Idempotency: re-importing should not duplicate questions
        let stats2 = import_all(&pool, root, data_dir).await.expect("import all 2");
        assert_eq!(stats2.questions, 54);

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM questions")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(count.0, 54);
    }
}

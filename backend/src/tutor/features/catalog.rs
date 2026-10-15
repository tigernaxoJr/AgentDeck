use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::tutor::shared::error::ApiError;
use crate::tutor::TutorState;

#[derive(Debug, Deserialize)]
pub struct CatalogQuery {
    pub student_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SubjectCatalogItem {
    pub id: String,
    pub name: String,
    pub book_title: String,
    pub units: Vec<UnitCatalogItem>,
}

#[derive(Debug, Serialize)]
pub struct UnitCatalogItem {
    pub id: String,
    pub title: String,
    pub total: i64,
    pub mastered: i64,
}

pub fn routes() -> Router<TutorState> {
    Router::new().route("/subjects", get(get_catalog))
}

async fn get_catalog(
    State(state): State<TutorState>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<Vec<SubjectCatalogItem>>, ApiError> {
    let subjects: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, name, book_title FROM subjects ORDER BY id ASC"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let mut result = Vec::new();

    for (s_id, s_name, s_book) in subjects {
        let units: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, title FROM units WHERE subject_id = ? ORDER BY ord ASC"
        )
        .bind(&s_id)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::from)?;

        let mut unit_items = Vec::new();

        for (u_id, u_title) in units {
            let total_row: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM questions WHERE unit_id = ? AND active = 1"
            )
            .bind(&u_id)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::from)?;

            let mastered_cnt: i64 = if let Some(sid) = query.student_id {
                let m_row: (i64,) = sqlx::query_as(
                    r#"
                    SELECT COUNT(*)
                    FROM questions q
                    JOIN mastery m ON m.question_id = q.id
                    WHERE q.unit_id = ? AND q.active = 1 AND m.student_id = ? AND m.box >= 3
                    "#
                )
                .bind(&u_id)
                .bind(sid)
                .fetch_one(&state.pool)
                .await
                .map_err(ApiError::from)?;
                m_row.0
            } else {
                0
            };

            unit_items.push(UnitCatalogItem {
                id: u_id,
                title: u_title,
                total: total_row.0,
                mastered: mastered_cnt,
            });
        }

        result.push(SubjectCatalogItem {
            id: s_id,
            name: s_name,
            book_title: s_book,
            units: unit_items,
        });
    }

    Ok(Json(result))
}

use sqlx::SqliteConnection;

use crate::tutor::ai::prompts::{PromptContext, PromptDialogItem, PromptWeakConcept};
use crate::tutor::domain::question::Question;
use crate::tutor::domain::session::Session;
use crate::tutor::store::load_weaknesses;

/// AI 呼叫是無狀態的：每次都從 DB 組出學童、科目、本題對話與弱點概念
pub async fn build_prompt_context(
    conn: &mut SqliteConnection,
    session: &Session,
    q: &Question,
    try_no: i64,
    choice: Option<String>,
    msg: Option<String>,
) -> Result<PromptContext, sqlx::Error> {
    let student_row: (String, i64) = sqlx::query_as("SELECT name, grade FROM students WHERE id = ?")
        .bind(session.student_id)
        .fetch_one(&mut *conn)
        .await?;

    let sub_row: Option<(String, String)> =
        sqlx::query_as("SELECT name, book_title FROM subjects WHERE id = ?")
            .bind(&session.subject_id)
            .fetch_optional(&mut *conn)
            .await?;

    let (subject_name, book_title) =
        sub_row.unwrap_or_else(|| (session.subject_id.clone(), "課本".into()));

    let dialog_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM dialog_turns WHERE session_id = ? AND question_id = ? ORDER BY id ASC LIMIT 10",
    )
    .bind(&session.id)
    .bind(&q.id)
    .fetch_all(&mut *conn)
    .await?;

    let dialog = dialog_rows
        .into_iter()
        .map(|(role, content)| PromptDialogItem { role, content })
        .collect();

    let weak_concepts = load_weaknesses(&mut *conn, session.student_id, 5)
        .await?
        .into_iter()
        .map(|w| PromptWeakConcept {
            tag: w.concept_tag,
            score: w.score,
            note: w.last_note,
        })
        .collect();

    Ok(PromptContext {
        student_name: student_row.0,
        student_grade: student_row.1,
        subject_name,
        book_title,
        question: q.clone(),
        try_no,
        dialog,
        weak_concepts,
        student_choice: choice,
        student_message: msg,
        q_state: session.q_state,
    })
}

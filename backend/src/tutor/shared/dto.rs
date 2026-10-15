use serde::{Deserialize, Serialize};
use crate::tutor::domain::question::{InputMode, Question};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookRef {
    pub page: i64,
    pub where_clause: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub id: String,
    pub qtype: String,
    pub input_mode: InputMode,
    pub section: String,
    pub stem: String,
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_page: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_where: Option<String>,
}

impl From<&Question> for QuestionView {
    fn from(q: &Question) -> Self {
        Self {
            id: q.id.clone(),
            qtype: serde_json::to_string(&q.qtype).unwrap_or_default().trim_matches('"').to_string(),
            input_mode: q.input_mode,
            section: q.section.clone(),
            stem: q.stem.clone(),
            options: q.options.clone(),
            char_count: q.char_count,
            image_url: q.image_path.as_ref().map(|p| format!("/api/tutor/assets/{p}")),
            book_page: q.book_page,
            book_where: q.book_where.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogTurnView {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_ref: Option<BookRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session_id: String,
    pub mode: String,
    pub index: usize,
    pub total: usize,
    pub q_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<QuestionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub dialog: Vec<DialogTurnView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealInfo {
    pub answer: String,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TutorReply {
    pub verdict: String,
    pub say: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_ref: Option<BookRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recognized_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followup_question: Option<String>,
    pub q_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal: Option<RevealInfo>,
    pub can_next: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_grade_available: Option<bool>,
}

pub mod prompts;
pub mod turn;
pub mod backends;

pub use backends::Backends;
pub use prompts::{build_prompt, PromptContext};
pub use turn::{call_turn, TutorFeedback};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use ai_cli::mock::MockEngine;
    use ai_cli::AiRawResult;
    use crate::tutor::ai::prompts::PromptContext;
    use crate::tutor::domain::question::{Answer, InputMode, QType, Question};
    use crate::tutor::domain::session::QState;
    use crate::tutor::shared::db::open_url;

    #[tokio::test]
    async fn test_turn_guards_and_logging() {
        let pool = open_url("sqlite::memory:").await.expect("open memory db");

        let q = Question {
            id: "q_guard".into(),
            subject_id: "social".into(),
            unit_id: "u1".into(),
            ord: 1,
            section: "".into(),
            qtype: QType::FillBlank,
            stem: "".into(),
            options: vec![],
            answer: Answer::Text("台北".into()),
            explanation: "解說".into(),
            keywords: vec![],
            char_count: None,
            image_path: None,
            book_page: Some(10),
            book_where: Some("頁中".into()),
            concept_tags: vec![],
            input_mode: InputMode::Text,
            source: "test".into(),
            active: true,
            updated_at: "".into(),
        };

        let ctx = PromptContext {
            student_name: "小明".into(),
            student_grade: 4,
            subject_name: "社會".into(),
            book_title: "課本".into(),
            question: q,
            try_no: 1,
            dialog: vec![],
            weak_concepts: vec![],
            student_choice: None,
            student_message: None,
            q_state: QState::Asking,
        };

        // Mock engine that outputs leaking answer and wrong book page
        let bad_turn_json = r#"{
            "verdict": "incorrect",
            "say": "答案其實是台北喔！",
            "action": "hint",
            "book_ref": { "page": 99, "where": "不知名角落" },
            "misconception": "沒記熟"
        }"#;

        let mock_engine = Arc::new(MockEngine::new(
            "test_mock",
            vec![Ok(AiRawResult {
                backend_id: "test_mock".into(),
                output_file: Some(bad_turn_json.into()),
                final_text: None,
                usage: None,
                duration: Duration::from_millis(50),
                stderr_tail: "".into(),
            })],
        ));

        let engines: Vec<Arc<dyn ai_cli::AiEngine>> = vec![mock_engine];
        let feedback = turn::call_turn(
            &engines,
            ai_cli::AiPurpose::TutorWrong,
            &ctx,
            vec![],
            vec![],
            &pool,
            Some("sess_test"),
            Duration::from_secs(5),
        )
        .await
        .expect("call_turn ok");

        // 1. Guard check: leaking answer was replaced with canned hint
        assert_eq!(feedback.say, "再想想看，可以翻翻課本喔！");
        // 2. Guard check: page 99 replaced with question.book_page (10)
        assert_eq!(feedback.book_ref.as_ref().map(|b| b.page), Some(10));

        // 3. Check ai_calls table in DB
        let call_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ai_calls")
            .fetch_one(&pool)
            .await
            .expect("fetch call count");
        assert_eq!(call_count.0, 1);
    }
}

use ai_cli::{Capabilities, VisionMode};
use serde::{Deserialize, Serialize};

use crate::tutor::domain::question::Question;
use crate::tutor::domain::session::QState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDialogItem {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptWeakConcept {
    pub tag: String,
    pub score: f64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptContext {
    pub student_name: String,
    pub student_grade: i64,
    pub subject_name: String,
    pub book_title: String,
    pub question: Question,
    pub try_no: i64,
    pub dialog: Vec<PromptDialogItem>,
    pub weak_concepts: Vec<PromptWeakConcept>,
    pub student_choice: Option<String>,
    pub student_message: Option<String>,
    pub q_state: QState,
}

pub fn image_ref(caps: &Capabilities, file: &str) -> String {
    match caps.vision {
        VisionMode::None => String::new(),
        VisionMode::PromptPath => format!("./{file}"),
        VisionMode::AtMention => format!("@{file}"),
        VisionMode::Flag { .. } => format!("附件圖片 {file}"),
    }
}

pub fn output_instruction(caps: &Capabilities) -> String {
    if caps.writes_files {
        "只把結果以 JSON 寫入 ./out/turn.json（UTF-8），格式如下，不要輸出其他檔案：".to_string()
    } else {
        "只輸出一個 JSON 物件，不要有任何其他文字或 markdown，格式如下：".to_string()
    }
}

pub fn context_instruction(caps: &Capabilities, ctx_json: &str) -> String {
    if caps.reads_files {
        "檔案 ./context.json 有題目、正確答案、解析、這題之前的對話、學童過去的弱點，請先讀它。".to_string()
    } else {
        format!("以下是題目、正確答案、解析、之前的對話與學童弱點：\n```json\n{}\n```", ctx_json)
    }
}

pub const AI_TURN_SCHEMA: &str = r#"{
  "verdict": "correct | partial | incorrect | unreadable | none",
  "recognized_text": "學童手寫辨識結果（沒有手寫就給空字串）",
  "say": "要對學童說的話，國小四年級用語，2~4 句，最多 120 字",
  "action": "praise | hint | explain | show_book | ask_followup | answer_question",
  "book_ref": { "page": 7, "where": "右上角的表格" },
  "misconception": "一句話描述學童可能的錯誤觀念；沒有就空字串",
  "concept_tags": ["方位判斷"],
  "followup_question": "可選：一個確認理解的小問題；沒有就空字串"
}"#;

pub fn build_context_json(ctx: &PromptContext) -> String {
    let q = &ctx.question;
    let obj = serde_json::json!({
        "student": {
            "name": ctx.student_name,
            "grade": ctx.student_grade,
        },
        "question": {
            "id": q.id,
            "qtype": serde_json::to_string(&q.qtype).unwrap().trim_matches('"'),
            "stem": q.stem,
            "options": q.options,
            "answer": q.answer,
            "explanation": q.explanation,
            "keywords": q.keywords,
            "book_page": q.book_page,
            "book_where": q.book_where,
            "has_image": q.image_path.is_some(),
        },
        "try_no": ctx.try_no,
        "dialog": ctx.dialog.iter().rev().take(10).rev().collect::<Vec<_>>(),
        "weak_concepts": ctx.weak_concepts.iter().take(5).collect::<Vec<_>>(),
    });

    serde_json::to_string_pretty(&obj).unwrap()
}

pub fn build_prompt(
    purpose: ai_cli::AiPurpose,
    ctx: &PromptContext,
    caps: &Capabilities,
) -> String {
    let ctx_json = build_context_json(ctx);

    let q_img_ref = if ctx.question.image_path.is_some() && caps.vision != VisionMode::None {
        image_ref(caps, "question.png")
    } else {
        String::new()
    };

    let rule2 = if !q_img_ref.is_empty() {
        format!(
            "2. 題目有圖（{}）時，你可以看圖理解，但「不要用文字描述圖片細節」；\n   需要學童看圖時，用 book_ref 叫學童翻到課本第幾頁的哪個位置，say 裡也要說出頁碼與位置。",
            q_img_ref
        )
    } else {
        "2. 這題的圖請一律用 book_ref 叫學童看課本。".to_string()
    };

    let sys_block = format!(
        r#"你是一位溫柔、有耐心的國小家教老師，正在陪一位{}年級的學童複習「{}」。
學童手上有實體課本/評量《{}》。

規則：
1. 用繁體中文、國小{}年級聽得懂的話，句子短，2~4 句。多鼓勵，不要說「你錯了」，改說「差一點點」。
{}
3. 給提示（hint）時絕對不能說出正確答案或答案中的字。
4. {}
5. {}
{}"#,
        ctx.student_grade,
        ctx.subject_name,
        ctx.book_title,
        ctx.student_grade,
        rule2,
        context_instruction(caps, &ctx_json),
        output_instruction(caps),
        AI_TURN_SCHEMA
    );

    let task_block = match purpose {
        ai_cli::AiPurpose::GradeHandwriting => {
            let ans_img = image_ref(caps, "answer.png");
            let qtype_specific = match ctx.question.qtype {
                crate::tutor::domain::question::QType::FillBlank => {
                    "同音字、錯別字都算錯，但要在 say 中指出是哪個字寫錯。"
                }
                crate::tutor::domain::question::QType::ChineseWrite => {
                    "檢查錯字、別字、漏字、多字、明顯多或少筆劃；不評筆順。指出錯在哪個字、哪個部件。"
                }
                crate::tutor::domain::question::QType::ShortAnswer => {
                    "意思對、包含關鍵概念即可算 correct；只對一部分算 partial。"
                }
                crate::tutor::domain::question::QType::MathExpr => {
                    "逐行辨識算式，找出第一個出錯的步驟（抄錯、進位、計算、單位）。過程錯但答案對算 partial。"
                }
                _ => "",
            };

            format!(
                r#"任務：{} 是學童用手寫板寫的答案。
1. 先辨識手寫內容填入 recognized_text；字跡無法辨識時 verdict=unreadable，請學童寫大一點、慢一點。
2. 與正確答案比對給 verdict。{}
3. 這是第 {} 次作答。答對 → action=praise，簡短說明為什麼對。
   第 1 次答錯 → action=hint 或 show_book，只給提示。第 2 次以上答錯 → action=explain，清楚講解並說出答案。"#,
                ans_img, qtype_specific, ctx.try_no
            )
        }
        ai_cli::AiPurpose::TutorWrong => {
            let choice_str = ctx.student_choice.as_deref().unwrap_or("未選擇");
            format!(
                r#"任務：學童選了「{}」，這是錯的（第 {} 次）。verdict 請填 incorrect。
依規則 3 給提示或講解。請推測學童的錯誤觀念寫在 misconception。"#,
                choice_str, ctx.try_no
            )
        }
        ai_cli::AiPurpose::StudentMessage => {
            let msg = ctx.student_message.as_deref().unwrap_or("");
            let q_state_str = serde_json::to_string(&ctx.q_state).unwrap().trim_matches('"').to_string();
            format!(
                r#"任務：學童對你說：「{}」。目前題目狀態：{}（若為 asking / hinting，不能洩漏答案）。
回答學童，verdict=none，action=answer_question 或 show_book。"#,
                msg, q_state_str
            )
        }
        ai_cli::AiPurpose::EnrichPage => {
            "任務：分析掃描頁中的題目位置與概念標籤。".to_string()
        }
    };

    format!("{}\n\n{}", sys_block, task_block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tutor::domain::question::{Answer, InputMode, QType};

    #[test]
    fn test_build_prompt_placeholders_and_capabilities() {
        let q = Question {
            id: "q1".into(),
            subject_id: "social".into(),
            unit_id: "u1".into(),
            ord: 1,
            section: "sec".into(),
            qtype: QType::FillBlank,
            stem: "看圖寫出方向".into(),
            options: vec![],
            answer: Answer::Text("東南".into()),
            explanation: "解說".into(),
            keywords: vec!["東南".into()],
            char_count: Some(2),
            image_path: Some("social/crops/test.png".into()),
            book_page: Some(5),
            book_where: Some("右上角".into()),
            concept_tags: vec!["方位".into()],
            input_mode: InputMode::CharGrid,
            source: "import".into(),
            active: true,
            updated_at: "".into(),
        };

        let ctx = PromptContext {
            student_name: "小明".into(),
            student_grade: 4,
            subject_name: "社會".into(),
            book_title: "四上 社會 評量".into(),
            question: q,
            try_no: 1,
            dialog: vec![],
            weak_concepts: vec![],
            student_choice: None,
            student_message: None,
            q_state: QState::Asking,
        };

        // 1. caps with AtMention and reads_files=false
        let caps = Capabilities {
            vision: VisionMode::AtMention,
            reads_files: false,
            writes_files: false,
            max_prompt_chars: 10000,
        };

        let prompt = build_prompt(ai_cli::AiPurpose::GradeHandwriting, &ctx, &caps);
        assert!(prompt.contains("@question.png"));
        assert!(prompt.contains("@answer.png"));
        assert!(prompt.contains("以下是題目、正確答案、解析"));
        assert!(!prompt.contains("{grade}"));
        assert!(!prompt.contains("{subject_name}"));
        assert!(!prompt.contains("{book_title}"));
        assert!(!prompt.contains("{context_instruction}"));
        assert!(!prompt.contains("{output_instruction}"));
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QType {
    TrueFalse,
    SingleChoice,
    FillBlank,
    ShortAnswer,
    MathExpr,
    ChineseWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    Choice,
    CharGrid,
    FreeCanvas,
    ImageCanvas,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Answer {
    TrueFalse(bool),
    Choice(usize),
    Text(String),
    Math {
        final_answer: String,
        steps_hint: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Correct,
    Partial,
    Incorrect,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookRef {
    pub page: i64,
    pub where_clause: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub subject_id: String,
    pub unit_id: String,
    pub ord: i64,
    pub section: String,
    pub qtype: QType,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: Answer,
    pub explanation: String,
    pub keywords: Vec<String>,
    pub char_count: Option<i64>,
    pub image_path: Option<String>,
    pub book_page: Option<i64>,
    pub book_where: Option<String>,
    pub concept_tags: Vec<String>,
    pub input_mode: InputMode,
    pub source: String,
    pub active: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceInput<'a> {
    /// 是非題：true = 對（O）、false = 錯（X）
    Bool(bool),
    /// 選擇題：0-based 選項索引
    Index(usize),
    Text(&'a str),
}

pub fn grade_choice(q: &Question, input: &ChoiceInput<'_>) -> Verdict {
    match (&q.answer, input) {
        (Answer::TrueFalse(expected), ChoiceInput::Bool(b)) => {
            if expected == b {
                Verdict::Correct
            } else {
                Verdict::Incorrect
            }
        }
        (Answer::TrueFalse(expected), ChoiceInput::Text(s)) => {
            let s = s.trim();
            let is_true = s.eq_ignore_ascii_case("O") || s == "○" || s == "o";
            let is_false = s.eq_ignore_ascii_case("X") || s == "×" || s == "x";
            if (is_true && *expected) || (is_false && !*expected) {
                Verdict::Correct
            } else {
                Verdict::Incorrect
            }
        }
        (Answer::Choice(expected_idx), ChoiceInput::Index(idx)) => {
            if expected_idx == idx {
                Verdict::Correct
            } else {
                Verdict::Incorrect
            }
        }
        (Answer::Choice(expected_idx), ChoiceInput::Text(s)) => {
            let s = s.trim();
            // 文字輸入的數字為 1-based（學童看到的題號）
            if let Ok(num) = s.parse::<usize>()
                && num >= 1 && *expected_idx == num - 1 {
                    return Verdict::Correct;
                }
            if let Some(opt_text) = q.options.get(*expected_idx)
                && opt_text.trim() == s {
                    return Verdict::Correct;
                }
            Verdict::Incorrect
        }
        _ => Verdict::Incorrect,
    }
}

fn tf_display(b: bool) -> &'static str {
    if b { "對" } else { "錯" }
}

fn choice_display(q: &Question, idx: usize) -> String {
    match q.options.get(idx) {
        Some(opt) => format!("{}. {}", idx + 1, opt),
        None => format!("{}", idx + 1),
    }
}

/// 學童的選擇轉成人看得懂的文字（給 AI prompt 與作答紀錄）
pub fn describe_choice(q: &Question, input: &ChoiceInput<'_>) -> String {
    match input {
        ChoiceInput::Bool(b) => tf_display(*b).to_string(),
        ChoiceInput::Index(idx) => choice_display(q, *idx),
        ChoiceInput::Text(s) => s.to_string(),
    }
}

pub fn answer_display(q: &Question) -> String {
    match &q.answer {
        Answer::TrueFalse(b) => tf_display(*b).to_string(),
        Answer::Choice(idx) => choice_display(q, *idx),
        Answer::Text(s) => s.clone(),
        Answer::Math { final_answer, .. } => final_answer.clone(),
    }
}

pub fn contains_answer(q: &Question, text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    match &q.answer {
        Answer::TrueFalse(_) => false, // O or X is too short and general
        Answer::Choice(idx) => {
            if let Some(opt) = q.options.get(*idx) {
                let trimmed = opt.trim();
                if trimmed.len() >= 2 && text.contains(trimmed) {
                    return true;
                }
            }
            false
        }
        Answer::Text(s) => {
            let trimmed = s.trim();
            trimmed.len() >= 2 && text.contains(trimmed)
        }
        Answer::Math { final_answer, .. } => {
            let trimmed = final_answer.trim();
            !trimmed.is_empty() && text.contains(trimmed)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RawImportQuestion {
    pub id: String,
    pub subject: String,
    pub unit: String,
    pub section: Option<String>,
    #[serde(rename = "type")]
    pub raw_type: String,
    pub stem: String,
    pub options: Option<Vec<String>>,
    pub answer: serde_json::Value,
    pub explanation: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub char_count: Option<i64>,
    pub image: Option<String>,
    #[serde(default)]
    pub need_handwriting: Option<bool>,
}

pub fn normalize_import(
    raw: RawImportQuestion,
    ord: i64,
    prev_book_page: Option<i64>,
    page_offset: i64,
    now: &str,
) -> Result<Question, String> {
    let subject_id = match raw.subject.trim() {
        "社會" => "social",
        "自然" => "nature",
        "國文" => "chinese",
        "數學" => "math",
        other => other,
    }
    .to_string();

    let options = raw.options.unwrap_or_default();

    let (qtype, input_mode, answer) = match raw.raw_type.as_str() {
        "true_false" => {
            let ans_str = raw
                .answer
                .as_str()
                .ok_or_else(|| format!("true_false answer not string: {:?}", raw.answer))?;
            let is_true = match ans_str.trim() {
                "O" | "o" | "○" => true,
                "X" | "x" | "×" => false,
                other => return Err(format!("unknown true_false value: {other}")),
            };
            (QType::TrueFalse, InputMode::Choice, Answer::TrueFalse(is_true))
        }
        "single_choice" => {
            let num = match &raw.answer {
                serde_json::Value::Number(n) => n
                    .as_i64()
                    .ok_or_else(|| format!("invalid single_choice num: {:?}", raw.answer))?,
                serde_json::Value::String(s) => s
                    .parse::<i64>()
                    .map_err(|e| format!("invalid single_choice num str: {s}: {e}"))?,
                other => return Err(format!("unknown single_choice answer type: {:?}", other)),
            };
            let zero_based = (num - 1).max(0) as usize;
            (QType::SingleChoice, InputMode::Choice, Answer::Choice(zero_based))
        }
        "handwrite_blank" => {
            let ans_str = match &raw.answer {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" / "),
                other => other.to_string(),
            };
            let mode = if raw.image.is_some() {
                InputMode::ImageCanvas
            } else {
                InputMode::CharGrid
            };
            (QType::FillBlank, mode, Answer::Text(ans_str))
        }
        "handwrite_short_answer" => {
            let ans_str = raw
                .answer
                .as_str()
                .unwrap_or_default()
                .to_string();
            (QType::ShortAnswer, InputMode::FreeCanvas, Answer::Text(ans_str))
        }
        "chinese_write" => {
            let ans_str = raw.answer.as_str().unwrap_or_default().to_string();
            (QType::ChineseWrite, InputMode::CharGrid, Answer::Text(ans_str))
        }
        "math_expr" => {
            let ans_str = raw.answer.as_str().unwrap_or_default().to_string();
            (
                QType::MathExpr,
                InputMode::FreeCanvas,
                Answer::Math {
                    final_answer: ans_str,
                    steps_hint: String::new(),
                },
            )
        }
        other => {
            return Err(format!("unsupported raw_type: {other}"));
        }
    };

    let mut book_page = None;
    if let Some(ref img) = raw.image {
        // e.g. crops/social_003_school_map.png
        if let Some(idx) = img.find('_') {
            let rest = &img[idx + 1..];
            if let Some(end) = rest.find('_')
                && let Ok(p) = rest[..end].parse::<i64>() {
                    book_page = Some(p + page_offset);
                }
        }
    }
    if book_page.is_none() {
        book_page = prev_book_page;
    }

    let mut concept_tags = Vec::new();
    if let Some(sec) = &raw.section
        && !sec.is_empty() {
            concept_tags.push(sec.clone());
        }
    let keywords = raw.keywords.unwrap_or_default();
    for kw in &keywords {
        if !concept_tags.contains(kw) {
            concept_tags.push(kw.clone());
        }
    }

    let image_path = raw.image.map(|img| {
        if img.starts_with("crops/") {
            format!("{}/{}", subject_id, img)
        } else {
            img
        }
    });

    Ok(Question {
        id: raw.id,
        subject_id,
        unit_id: raw.unit,
        ord,
        section: raw.section.unwrap_or_default(),
        qtype,
        stem: raw.stem,
        options,
        answer,
        explanation: raw.explanation.unwrap_or_default(),
        keywords,
        char_count: raw.char_count,
        image_path,
        book_page,
        book_where: None,
        concept_tags,
        input_mode,
        source: "import".to_string(),
        active: true,
        updated_at: now.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grade_choice_tf() {
        let q = Question {
            id: "tf_1".into(),
            subject_id: "social".into(),
            unit_id: "u1".into(),
            ord: 1,
            section: "".into(),
            qtype: QType::TrueFalse,
            stem: "測試是非題".into(),
            options: vec![],
            answer: Answer::TrueFalse(true),
            explanation: "".into(),
            keywords: vec![],
            char_count: None,
            image_path: None,
            book_page: None,
            book_where: None,
            concept_tags: vec![],
            input_mode: InputMode::Choice,
            source: "import".into(),
            active: true,
            updated_at: "".into(),
        };
        assert_eq!(grade_choice(&q, &ChoiceInput::Bool(true)), Verdict::Correct);
        assert_eq!(grade_choice(&q, &ChoiceInput::Bool(false)), Verdict::Incorrect);
        assert_eq!(grade_choice(&q, &ChoiceInput::Text("O")), Verdict::Correct);
        assert_eq!(grade_choice(&q, &ChoiceInput::Text("○")), Verdict::Correct);
        assert_eq!(grade_choice(&q, &ChoiceInput::Text("X")), Verdict::Incorrect);
    }

    #[test]
    fn test_grade_choice_mc() {
        let q = Question {
            id: "mc_1".into(),
            subject_id: "social".into(),
            unit_id: "u1".into(),
            ord: 1,
            section: "".into(),
            qtype: QType::SingleChoice,
            stem: "測試選擇題".into(),
            options: vec!["選項一".into(), "選項二".into()],
            answer: Answer::Choice(1),
            explanation: "".into(),
            keywords: vec![],
            char_count: None,
            image_path: None,
            book_page: None,
            book_where: None,
            concept_tags: vec![],
            input_mode: InputMode::Choice,
            source: "import".into(),
            active: true,
            updated_at: "".into(),
        };
        assert_eq!(grade_choice(&q, &ChoiceInput::Index(1)), Verdict::Correct);
        assert_eq!(grade_choice(&q, &ChoiceInput::Index(0)), Verdict::Incorrect);
        assert_eq!(grade_choice(&q, &ChoiceInput::Text("選項二")), Verdict::Correct);
        assert_eq!(grade_choice(&q, &ChoiceInput::Text("2")), Verdict::Correct);
        assert_eq!(grade_choice(&q, &ChoiceInput::Text("1")), Verdict::Incorrect);
        assert_eq!(describe_choice(&q, &ChoiceInput::Index(1)), "2. 選項二");
    }

    #[test]
    fn test_contains_answer() {
        let q = Question {
            id: "q1".into(),
            subject_id: "social".into(),
            unit_id: "u1".into(),
            ord: 1,
            section: "".into(),
            qtype: QType::FillBlank,
            stem: "".into(),
            options: vec![],
            answer: Answer::Text("台北101".into()),
            explanation: "".into(),
            keywords: vec![],
            char_count: None,
            image_path: None,
            book_page: None,
            book_where: None,
            concept_tags: vec![],
            input_mode: InputMode::CharGrid,
            source: "import".into(),
            active: true,
            updated_at: "".into(),
        };
        assert!(contains_answer(&q, "答案就在台北101裡面"));
        assert!(!contains_answer(&q, "答案在台中"));
    }

    #[test]
    fn test_normalize_import_mc_and_map() {
        let raw_mc = RawImportQuestion {
            id: "soc_01_mc_1".into(),
            subject: "社會".into(),
            unit: "social_01".into(),
            section: Some("方位判斷".into()),
            raw_type: "single_choice".into(),
            stem: "若站在升旗台...".into(),
            options: Some(vec!["東".into(), "西".into(), "南".into(), "北".into()]),
            answer: serde_json::json!(2), // 1-based index 2 -> 0-based 1
            explanation: Some("解釋".into()),
            keywords: Some(vec!["升旗台".into()]),
            char_count: None,
            image: None,
            need_handwriting: None,
        };
        let q_mc = normalize_import(raw_mc, 1, None, 0, "2026-10-15T00:00:00Z").unwrap();
        assert_eq!(q_mc.answer, Answer::Choice(1));
        assert_eq!(q_mc.qtype, QType::SingleChoice);
        assert_eq!(q_mc.input_mode, InputMode::Choice);

        let raw_map = RawImportQuestion {
            id: "soc_01_map_1".into(),
            subject: "社會".into(),
            unit: "social_01".into(),
            section: Some("方位判斷".into()),
            raw_type: "handwrite_blank".into(),
            stem: "看圖作答...".into(),
            options: None,
            answer: serde_json::json!("東北"),
            explanation: Some("解釋".into()),
            keywords: Some(vec!["地圖".into()]),
            char_count: Some(2),
            image: Some("crops/social_003_school_map.png".into()),
            need_handwriting: Some(true),
        };
        let q_map = normalize_import(raw_map, 2, None, 0, "2026-10-15T00:00:00Z").unwrap();
        assert_eq!(q_map.book_page, Some(3));
        assert_eq!(q_map.input_mode, InputMode::ImageCanvas);
        assert_eq!(q_map.answer, Answer::Text("東北".into()));
    }
}

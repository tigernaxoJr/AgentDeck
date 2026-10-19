use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mastery {
    pub student_id: i64,
    pub question_id: String,
    pub box_level: i64, // 0..5
    pub due_at: DateTime<Utc>,
    pub streak: i64,
    pub total_correct: i64,
    pub total_wrong: i64,
    pub last_verdict: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptWeakness {
    pub student_id: i64,
    pub concept_tag: String,
    pub score: f64,
    pub wrong_count: i64,
    pub last_note: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    FirstTry,
    AfterHint,
    Failed,
}

pub fn get_interval(box_level: i64) -> Duration {
    match box_level {
        0 => Duration::minutes(10),
        1 => Duration::days(1),
        2 => Duration::days(2),
        3 => Duration::days(4),
        4 => Duration::days(7),
        _ => Duration::days(14),
    }
}

pub fn next_mastery(
    prev: Option<Mastery>,
    student_id: i64,
    question_id: &str,
    outcome: Outcome,
    now: DateTime<Utc>,
) -> Mastery {
    let base = prev.unwrap_or_else(|| Mastery {
        student_id,
        question_id: question_id.to_string(),
        box_level: 0,
        due_at: now,
        streak: 0,
        total_correct: 0,
        total_wrong: 0,
        last_verdict: None,
        last_seen_at: None,
    });

    match outcome {
        Outcome::FirstTry => {
            let next_box = (base.box_level + 1).min(5);
            let next_streak = base.streak + 1;
            let due = now + get_interval(next_box);
            Mastery {
                student_id,
                question_id: question_id.to_string(),
                box_level: next_box,
                due_at: due,
                streak: next_streak,
                total_correct: base.total_correct + 1,
                total_wrong: base.total_wrong,
                last_verdict: Some("correct".to_string()),
                last_seen_at: Some(now),
            }
        }
        Outcome::AfterHint => {
            // max(box-1, 0) 但至少 1 -> (base.box_level - 1).clamp(1, 5) if base >= 2 else 1
            let next_box = (base.box_level - 1).clamp(1, 5);
            let due = now + Duration::days(1);
            Mastery {
                student_id,
                question_id: question_id.to_string(),
                box_level: next_box,
                due_at: due,
                streak: 0,
                total_correct: base.total_correct + 1,
                total_wrong: base.total_wrong + 1,
                last_verdict: Some("partial_correct".to_string()),
                last_seen_at: Some(now),
            }
        }
        Outcome::Failed => {
            let due = now + Duration::minutes(10);
            Mastery {
                student_id,
                question_id: question_id.to_string(),
                box_level: 0,
                due_at: due,
                streak: 0,
                total_correct: base.total_correct,
                total_wrong: base.total_wrong + 1,
                last_verdict: Some("incorrect".to_string()),
                last_seen_at: Some(now),
            }
        }
    }
}

pub fn update_weakness(
    prev: Option<ConceptWeakness>,
    student_id: i64,
    concept_tag: &str,
    outcome: Outcome,
    note: Option<String>,
    now: DateTime<Utc>,
) -> ConceptWeakness {
    let mut item = prev.unwrap_or_else(|| ConceptWeakness {
        student_id,
        concept_tag: concept_tag.to_string(),
        score: 0.0,
        wrong_count: 0,
        last_note: None,
        updated_at: now,
    });

    match outcome {
        Outcome::Failed => {
            item.score += 1.0;
            item.wrong_count += 1;
            if note.is_some() {
                item.last_note = note;
            }
        }
        Outcome::AfterHint => {
            item.score += 0.5;
            item.wrong_count += 1;
            if note.is_some() {
                item.last_note = note;
            }
        }
        Outcome::FirstTry => {
            item.score *= 0.6;
            if item.score < 0.2 {
                item.score = 0.0;
            }
        }
    }
    item.updated_at = now;
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mastery_first_try() {
        let now = Utc::now();
        let m = next_mastery(None, 1, "q1", Outcome::FirstTry, now);
        assert_eq!(m.box_level, 1);
        assert_eq!(m.streak, 1);
        assert_eq!(m.total_correct, 1);
        assert_eq!(m.total_wrong, 0);
        assert_eq!(m.due_at, now + Duration::days(1));

        let m2 = next_mastery(Some(m), 1, "q1", Outcome::FirstTry, now);
        assert_eq!(m2.box_level, 2);
        assert_eq!(m2.streak, 2);
        assert_eq!(m2.due_at, now + Duration::days(2));
    }

    #[test]
    fn test_mastery_after_hint() {
        let now = Utc::now();
        let prev = Mastery {
            student_id: 1,
            question_id: "q1".into(),
            box_level: 3,
            due_at: now,
            streak: 3,
            total_correct: 3,
            total_wrong: 0,
            last_verdict: None,
            last_seen_at: None,
        };
        let m = next_mastery(Some(prev), 1, "q1", Outcome::AfterHint, now);
        assert_eq!(m.box_level, 2);
        assert_eq!(m.streak, 0);
        assert_eq!(m.due_at, now + Duration::days(1));
    }

    #[test]
    fn test_mastery_failed() {
        let now = Utc::now();
        let prev = Mastery {
            student_id: 1,
            question_id: "q1".into(),
            box_level: 4,
            due_at: now,
            streak: 4,
            total_correct: 4,
            total_wrong: 0,
            last_verdict: None,
            last_seen_at: None,
        };
        let m = next_mastery(Some(prev), 1, "q1", Outcome::Failed, now);
        assert_eq!(m.box_level, 0);
        assert_eq!(m.streak, 0);
        assert_eq!(m.due_at, now + Duration::minutes(10));
    }

    #[test]
    fn test_concept_weakness_update() {
        let now = Utc::now();
        let w = update_weakness(None, 1, "方位判斷", Outcome::Failed, Some("誤判".into()), now);
        assert_eq!(w.score, 1.0);
        assert_eq!(w.wrong_count, 1);
        assert_eq!(w.last_note, Some("誤判".into()));

        let w2 = update_weakness(Some(w), 1, "方位判斷", Outcome::FirstTry, None, now);
        assert!((w2.score - 0.6).abs() < 1e-6);

        let w3 = update_weakness(Some(w2), 1, "方位判斷", Outcome::FirstTry, None, now);
        assert!((w3.score - 0.36).abs() < 1e-6);

        let w4 = update_weakness(Some(w3), 1, "方位判斷", Outcome::FirstTry, None, now);
        // 0.36 * 0.6 = 0.216 > 0.2
        let w5 = update_weakness(Some(w4), 1, "方位判斷", Outcome::FirstTry, None, now);
        // 0.216 * 0.6 = 0.1296 < 0.2 -> 0.0
        assert_eq!(w5.score, 0.0);
    }
}

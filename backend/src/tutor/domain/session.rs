use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::progress::Outcome;
use super::question::Verdict;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QState {
    Asking,
    Hinting,
    Revealed,
    Explained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeachAction {
    Praise,
    Hint,
    Explain,
    RetryUnreadable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub state: QState,
    pub action: TeachAction,
    pub reveal: bool,
    pub outcome: Option<Outcome>,
    pub requeued: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub student_id: i64,
    pub mode: String,
    pub subject_id: String,
    pub unit_id: Option<String>,
    pub queue: Vec<String>,
    pub cursor: usize,
    pub q_state: QState,
    pub q_try: i64,
    pub q_hinted: bool,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

impl Session {
    pub fn current_question_id(&self) -> Option<&str> {
        self.queue.get(self.cursor).map(|s| s.as_str())
    }

    pub fn try_no(&self) -> i64 {
        self.q_try
    }

    pub fn apply_verdict(&mut self, v: Verdict, _now: DateTime<Utc>) -> Step {
        match v {
            Verdict::Unreadable => Step {
                state: self.q_state,
                action: TeachAction::RetryUnreadable,
                reveal: false,
                outcome: None,
                requeued: false,
            },
            Verdict::Correct => {
                let outcome = if self.q_try == 0 {
                    Outcome::FirstTry
                } else {
                    Outcome::AfterHint
                };
                self.q_try += 1;
                self.q_state = QState::Revealed;
                Step {
                    state: QState::Revealed,
                    action: TeachAction::Praise,
                    reveal: true,
                    outcome: Some(outcome),
                    requeued: false,
                }
            }
            Verdict::Incorrect | Verdict::Partial => {
                self.q_try += 1;
                if self.q_try == 1 {
                    self.q_state = QState::Hinting;
                    self.q_hinted = true;
                    Step {
                        state: QState::Hinting,
                        action: TeachAction::Hint,
                        reveal: false,
                        outcome: None,
                        requeued: false,
                    }
                } else {
                    // q_try >= 2: explain and reveal
                    self.q_state = QState::Explained;
                    let mut requeued = false;
                    if let Some(curr_id) = self.current_question_id() {
                        let count = self.queue.iter().filter(|&id| id == curr_id).count();
                        if count < 2 {
                            self.queue.push(curr_id.to_string());
                            requeued = true;
                        }
                    }
                    Step {
                        state: QState::Explained,
                        action: TeachAction::Explain,
                        reveal: true,
                        outcome: Some(Outcome::Failed),
                        requeued,
                    }
                }
            }
        }
    }

    pub fn can_next(&self) -> bool {
        matches!(self.q_state, QState::Revealed | QState::Explained)
    }

    pub fn advance(&mut self) -> bool {
        if !self.can_next() {
            return false;
        }
        if self.cursor + 1 < self.queue.len() {
            self.cursor += 1;
            self.q_state = QState::Asking;
            self.q_try = 0;
            self.q_hinted = false;
            true
        } else {
            false
        }
    }

    pub fn finish(&mut self, now: DateTime<Utc>) {
        self.ended_at = Some(now);
    }
}

pub struct Candidates {
    pub progress_ids: Vec<String>,
    pub due_ids: Vec<String>,
    pub weak_concept_ids: Vec<String>,
    pub wrong_ids: Vec<String>,
}

pub fn select_queue(mode: &str, candidates: &Candidates, n: usize, seed: u64) -> Vec<String> {
    match mode {
        "progress" => {
            let mut list = candidates.progress_ids.clone();
            list.truncate(n);
            list
        }
        "review" => {
            let mut result = Vec::new();
            for id in &candidates.due_ids {
                if !result.contains(id) {
                    result.push(id.clone());
                    if result.len() >= n {
                        break;
                    }
                }
            }
            if result.len() < n {
                for id in &candidates.weak_concept_ids {
                    if !result.contains(id) {
                        result.push(id.clone());
                        if result.len() >= n {
                            break;
                        }
                    }
                }
            }
            if result.len() < n {
                for id in &candidates.wrong_ids {
                    if !result.contains(id) {
                        result.push(id.clone());
                        if result.len() >= n {
                            break;
                        }
                    }
                }
            }
            if result.is_empty() {
                return result;
            }
            // Deterministic shuffle with seed
            shuffle(&mut result, seed);
            result
        }
        "mixed" => {
            let review_n = ((n as f64) * 0.6).ceil() as usize;
            let progress_n = n.saturating_sub(review_n);

            let mut reviews = select_queue("review", candidates, review_n, seed);
            let mut progresses = Vec::new();
            for id in &candidates.progress_ids {
                if !reviews.contains(id) {
                    progresses.push(id.clone());
                    if progresses.len() >= progress_n {
                        break;
                    }
                }
            }
            reviews.extend(progresses);
            reviews
        }
        _ => candidates.progress_ids.iter().take(n).cloned().collect(),
    }
}

fn shuffle<T>(slice: &mut [T], mut seed: u64) {
    if slice.len() <= 1 {
        return;
    }
    // simple LCG pseudo-random generator
    for i in (1..slice.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (seed >> 32) as usize % (i + 1);
        slice.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_machine() {
        let now = Utc::now();
        let mut session = Session {
            id: "s1".into(),
            student_id: 1,
            mode: "progress".into(),
            subject_id: "social".into(),
            unit_id: Some("u1".into()),
            queue: vec!["q1".into(), "q2".into()],
            cursor: 0,
            q_state: QState::Asking,
            q_try: 0,
            q_hinted: false,
            started_at: now,
            ended_at: None,
        };

        // 1. Unreadable does not increment q_try
        let step_unreadable = session.apply_verdict(Verdict::Unreadable, now);
        assert_eq!(step_unreadable.action, TeachAction::RetryUnreadable);
        assert_eq!(session.q_try, 0);

        // 2. First try wrong -> Hinting, reveal=false
        let step_hint = session.apply_verdict(Verdict::Incorrect, now);
        assert_eq!(step_hint.action, TeachAction::Hint);
        assert_eq!(session.q_state, QState::Hinting);
        assert!(!step_hint.reveal);
        assert_eq!(session.q_try, 1);
        assert_eq!(session.queue.len(), 2);

        // 3. Second try wrong -> Explained, reveal=true, requeued=true
        let step_explain = session.apply_verdict(Verdict::Incorrect, now);
        assert_eq!(step_explain.action, TeachAction::Explain);
        assert_eq!(session.q_state, QState::Explained);
        assert!(step_explain.reveal);
        assert!(step_explain.requeued);
        assert_eq!(session.queue.len(), 3);
        assert_eq!(session.queue[2], "q1");

        assert!(session.can_next());
        assert!(session.advance());
        assert_eq!(session.cursor, 1);
        assert_eq!(session.current_question_id(), Some("q2"));
        assert_eq!(session.q_state, QState::Asking);

        // 4. First try correct on q2 -> Revealed, Praise, FirstTry
        let step_correct = session.apply_verdict(Verdict::Correct, now);
        assert_eq!(step_correct.action, TeachAction::Praise);
        assert_eq!(session.q_state, QState::Revealed);
        assert!(step_correct.reveal);
        assert_eq!(step_correct.outcome, Some(Outcome::FirstTry));
    }

    #[test]
    fn test_select_queue() {
        let candidates = Candidates {
            progress_ids: vec!["p1".into(), "p2".into(), "p3".into()],
            due_ids: vec!["d1".into()],
            weak_concept_ids: vec!["w1".into(), "w2".into()],
            wrong_ids: vec!["wr1".into()],
        };

        let q_prog = select_queue("progress", &candidates, 2, 42);
        assert_eq!(q_prog, vec!["p1", "p2"]);

        let q_rev = select_queue("review", &candidates, 3, 42);
        assert_eq!(q_rev.len(), 3);
        assert!(q_rev.contains(&"d1".to_string()));
        assert!(q_rev.contains(&"w1".to_string()));
        assert!(q_rev.contains(&"w2".to_string()));

        let empty_cand = Candidates {
            progress_ids: vec![],
            due_ids: vec![],
            weak_concept_ids: vec![],
            wrong_ids: vec![],
        };
        assert!(select_queue("review", &empty_cand, 5, 42).is_empty());
    }
}

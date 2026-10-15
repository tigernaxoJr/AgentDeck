use std::time::Duration;
use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use ai_cli::{AiPurpose, RunFile};
use crate::tutor::ai::prompts::{PromptContext, PromptDialogItem, PromptWeakConcept};
use crate::tutor::ai::turn::{
    call_turn, canned_praise, canned_wrong_explain, canned_wrong_hint, TutorFeedback,
};
use crate::tutor::domain::progress::{next_mastery, update_weakness, Outcome};
use crate::tutor::domain::question::{
    answer_display, grade_choice, ChoiceInput, InputMode, Question, Verdict,
};
use crate::tutor::domain::session::{QState, Session, Step, TeachAction};
use crate::tutor::shared::dto::{BookRef, RevealInfo, TutorReply};
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{
    load_progress, load_question, load_session, load_weaknesses, save_progress, save_session,
    save_weakness,
};
use crate::tutor::TutorState;

#[derive(Debug, Deserialize)]
pub struct AnswerReq {
    pub question_id: String,
    pub kind: String, // 'choice' | 'text' | 'handwriting'
    pub choice: Option<serde_json::Value>, // int or string
    pub text: Option<String>,
    pub image_png_base64: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SelfGradeReq {
    pub question_id: String,
    pub correct: bool,
}

pub fn routes() -> Router<TutorState> {
    Router::new()
        .route("/sessions/{id}/answer", post(handle_answer))
        .route("/sessions/{id}/self_grade", post(handle_self_grade))
}

struct SessionUnlocker {
    sid: String,
    locks: std::sync::Arc<dashmap::DashMap<String, ()>>,
}

impl Drop for SessionUnlocker {
    fn drop(&mut self) {
        self.locks.remove(&self.sid);
    }
}

async fn handle_answer(
    State(state): State<TutorState>,
    Path(id): Path<String>,
    Json(payload): Json<AnswerReq>,
) -> Result<Json<TutorReply>, ApiError> {
    // 1. Session concurrency lock
    if state.session_locks.insert(id.clone(), ()).is_some() {
        return Err(ApiError::Conflict("Another request is processing for this session".into()));
    }
    let _guard = SessionUnlocker {
        sid: id.clone(),
        locks: state.session_locks.clone(),
    };

    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let mut session = load_session(&mut *conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    if session.current_question_id() != Some(&payload.question_id) {
        return Err(ApiError::BadRequest("Question ID does not match current question".into()));
    }

    if !matches!(session.q_state, QState::Asking | QState::Hinting) {
        return Err(ApiError::Conflict("Current question is already completed".into()));
    }

    let q = load_question(&mut *conn, &payload.question_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;

    let now = Utc::now();
    let attempt_id = Uuid::new_v4().to_string();

    // 2. Decode image if handwriting
    let mut input_image_path = None;
    let mut image_bytes = None;
    if payload.kind == "handwriting" {
        if let Some(ref b64) = payload.image_png_base64 {
            let clean_b64 = b64.trim().trim_start_matches("data:image/png;base64,");
            let raw_bytes = base64_simd_or_standard_decode(clean_b64)
                .map_err(|e| ApiError::BadRequest(format!("Invalid base64 image: {e}")))?;

            if raw_bytes.len() > 3 * 1024 * 1024 {
                return Err(ApiError::BadRequest("Image exceeds 3MB limit".into()));
            }
            // PNG magic bytes: 89 50 4E 47 0D 0A 1A 0A
            if raw_bytes.len() < 8 || &raw_bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
                return Err(ApiError::BadRequest("Image must be valid PNG format".into()));
            }

            let file_rel = format!("answers/{attempt_id}.png");
            let file_abs = state.data_dir.join(&file_rel);
            if let Some(parent) = file_abs.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            tokio::fs::write(&file_abs, &raw_bytes)
                .await
                .map_err(|e| ApiError::Internal(format!("Failed to save answer image: {e}")))?;

            input_image_path = Some(file_rel);
            image_bytes = Some(raw_bytes);
        } else {
            return Err(ApiError::BadRequest("Missing image for handwriting".into()));
        }
    }

    // 3. Grading (Rule vs AI)
    let is_rule_gradable = matches!(payload.kind.as_str(), "choice")
        || (payload.kind == "text" && matches!(q.qtype, crate::tutor::domain::question::QType::TrueFalse | crate::tutor::domain::question::QType::SingleChoice));

    let mut graded_by = "rule";
    let mut verdict = Verdict::Incorrect;
    let mut recognized_text = None;
    let mut ai_feedback: Option<TutorFeedback> = None;
    let mut self_grade_available = false;

    if is_rule_gradable {
        let input = match &payload.choice {
            Some(serde_json::Value::Number(n)) => {
                ChoiceInput::Index(n.as_i64().unwrap_or(0).max(0) as usize)
            }
            Some(serde_json::Value::String(s)) => ChoiceInput::Text(s),
            _ => {
                if let Some(ref t) = payload.text {
                    ChoiceInput::Text(t)
                } else {
                    ChoiceInput::Text("")
                }
            }
        };
        verdict = grade_choice(&q, &input);
    } else {
        // Need AI grading
        let vision_engines = state.ai.engines_for(AiPurpose::GradeHandwriting).await;
        if vision_engines.is_empty() {
            // No vision engine available -> fallback to self-grade
            self_grade_available = true;
            return Ok(Json(TutorReply {
                verdict: "none".into(),
                say: "目前 AI 辨識尚未連線，請點選標準答案自己對答案喔！".into(),
                action: "self_grade".into(),
                book_ref: None,
                recognized_text: None,
                followup_question: None,
                q_state: serde_json::to_string(&session.q_state).unwrap().trim_matches('"').into(),
                reveal: Some(RevealInfo {
                    answer: answer_display(&q),
                    explanation: q.explanation.clone(),
                }),
                can_next: false,
                self_grade_available: Some(true),
            }));
        }

        let ctx = build_prompt_context(&mut conn, &session, &q, None, None).await?;
        let mut run_files = vec![
            RunFile {
                name: "context.json".into(),
                bytes: crate::tutor::ai::prompts::build_context_json(&ctx).into_bytes(),
            },
        ];
        let mut images = Vec::new();

        if let Some(bytes) = image_bytes {
            run_files.push(RunFile {
                name: "answer.png".into(),
                bytes,
            });
            images.push("answer.png".into());
        }

        if let Some(ref img_path) = q.image_path {
            let full_p = state.data_dir.join("assets").join(img_path);
            if full_p.exists() {
                if let Ok(bytes) = tokio::fs::read(&full_p).await {
                    run_files.push(RunFile {
                        name: "question.png".into(),
                        bytes,
                    });
                    images.push("question.png".into());
                }
            }
        }

        // Concurrency semaphore
        let _permit = state.ai_sem.acquire().await.map_err(|e| ApiError::Internal(e.to_string()))?;

        let fb = call_turn(
            &vision_engines,
            AiPurpose::GradeHandwriting,
            &ctx,
            run_files,
            images,
            &state.pool,
            Some(&session.id),
            Duration::from_secs(90),
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        verdict = fb.verdict;
        recognized_text = Some(fb.recognized_text.clone());
        ai_feedback = Some(fb);
        graded_by = "ai";
    }

    // 4. Session state transition
    let step = session.apply_verdict(verdict, now);

    // 5. Teaching AI if wrong & rule-graded
    let mut tutor_say = String::new();
    let mut tutor_action = serde_json::to_string(&step.action).unwrap().trim_matches('"').to_string();
    let mut book_ref = None;
    let mut followup_q = None;

    if let Some(ref fb) = ai_feedback {
        tutor_say = fb.say.clone();
        book_ref = fb.book_ref.clone();
        followup_q = fb.followup_question.clone();
    } else {
        match step.action {
            TeachAction::Praise => {
                tutor_say = canned_praise().to_string();
            }
            TeachAction::Hint | TeachAction::Explain => {
                // Call AI for wrong choice/explanation if engines available
                let engines = state.ai.engines_for(AiPurpose::TutorWrong).await;
                if !engines.is_empty() {
                    let choice_str = payload.choice.as_ref().map(|v| v.to_string()).or_else(|| payload.text.clone());
                    let ctx = build_prompt_context(&mut conn, &session, &q, choice_str, None).await?;
                    let run_files = vec![
                        RunFile {
                            name: "context.json".into(),
                            bytes: crate::tutor::ai::prompts::build_context_json(&ctx).into_bytes(),
                        },
                    ];
                    let _permit = state.ai_sem.acquire().await.map_err(|e| ApiError::Internal(e.to_string()))?;
                    if let Ok(fb) = call_turn(
                        &engines,
                        AiPurpose::TutorWrong,
                        &ctx,
                        run_files,
                        vec![],
                        &state.pool,
                        Some(&session.id),
                        Duration::from_secs(90),
                    ).await {
                        tutor_say = fb.say;
                        book_ref = fb.book_ref;
                        followup_q = fb.followup_question;
                    }
                }

                if tutor_say.is_empty() {
                    let default_br = q.book_page.map(|p| BookRef {
                        page: p,
                        where_clause: q.book_where.clone().unwrap_or_default(),
                    });
                    if step.action == TeachAction::Hint {
                        tutor_say = canned_wrong_hint(default_br.as_ref());
                        book_ref = default_br;
                    } else {
                        tutor_say = canned_wrong_explain(&answer_display(&q), &q.explanation);
                        book_ref = default_br;
                    }
                }
            }
            TeachAction::RetryUnreadable => {
                tutor_say = "字跡有點不清楚，請寫大一點、慢一點再試一次喔！".to_string();
            }
        }
    }

    // 6. Persist in single transaction
    let mut tx = state.pool.begin().await.map_err(ApiError::from)?;

    // Insert attempt
    let verdict_str = serde_json::to_string(&verdict).unwrap().trim_matches('"').to_string();
    let input_text = payload.choice.as_ref().map(|v| v.to_string()).or(payload.text);
    sqlx::query(
        r#"
        INSERT INTO attempts (
            id, session_id, student_id, question_id, try_no, input_kind,
            input_text, input_image_path, recognized_text, verdict, graded_by, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&attempt_id)
    .bind(&session.id)
    .bind(session.student_id)
    .bind(&q.id)
    .bind(session.q_try)
    .bind(&payload.kind)
    .bind(input_text)
    .bind(input_image_path)
    .bind(&recognized_text)
    .bind(&verdict_str)
    .bind(graded_by)
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::from)?;

    // Insert dialog turn
    let br_json = book_ref.as_ref().map(|b| serde_json::to_string(b).unwrap());
    sqlx::query(
        r#"
        INSERT INTO dialog_turns (
            session_id, question_id, role, content, action, book_ref_json, created_at
        ) VALUES (?, ?, 'tutor', ?, ?, ?, ?)
        "#,
    )
    .bind(&session.id)
    .bind(&q.id)
    .bind(&tutor_say)
    .bind(&tutor_action)
    .bind(br_json)
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::from)?;

    // Save session
    save_session(&mut *tx, &session).await.map_err(ApiError::from)?;

    // Update mastery & weakness if outcome reached
    if let Some(outcome) = step.outcome {
        let prev_m = load_progress(&mut *tx, session.student_id, &q.id).await.map_err(ApiError::from)?;
        let next_m = next_mastery(prev_m, session.student_id, &q.id, outcome, now);
        save_progress(&mut *tx, &next_m).await.map_err(ApiError::from)?;

        let note = ai_feedback.as_ref().and_then(|f| f.misconception.clone());
        for tag in &q.concept_tags {
            let row: Option<(f64, i64, Option<String>)> = sqlx::query_as(
                "SELECT score, wrong_count, last_note FROM concept_weakness WHERE student_id = ? AND concept_tag = ?"
            )
            .bind(session.student_id)
            .bind(tag)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::from)?;

            let prev_w = row.map(|(score, wrong_count, last_note)| {
                crate::tutor::domain::progress::ConceptWeakness {
                    student_id: session.student_id,
                    concept_tag: tag.clone(),
                    score,
                    wrong_count,
                    last_note,
                    updated_at: now,
                }
            });

            let next_w = update_weakness(prev_w, session.student_id, tag, outcome, note.clone(), now);
            save_weakness(&mut *tx, &next_w).await.map_err(ApiError::from)?;
        }
    }

    tx.commit().await.map_err(ApiError::from)?;

    let reveal = if step.reveal {
        Some(RevealInfo {
            answer: answer_display(&q),
            explanation: q.explanation.clone(),
        })
    } else {
        None
    };

    let q_state_str = serde_json::to_string(&session.q_state).unwrap().trim_matches('"').to_string();

    Ok(Json(TutorReply {
        verdict: verdict_str,
        say: tutor_say,
        action: tutor_action,
        book_ref,
        recognized_text,
        followup_question: followup_q,
        q_state: q_state_str,
        reveal,
        can_next: session.can_next(),
        self_grade_available: Some(self_grade_available),
    }))
}

async fn handle_self_grade(
    State(state): State<TutorState>,
    Path(id): Path<String>,
    Json(payload): Json<SelfGradeReq>,
) -> Result<Json<TutorReply>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    let mut session = load_session(&mut *conn, &id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    if session.current_question_id() != Some(&payload.question_id) {
        return Err(ApiError::BadRequest("Question ID does not match current question".into()));
    }

    let q = load_question(&mut *conn, &payload.question_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;

    let now = Utc::now();
    let verdict = if payload.correct {
        Verdict::Correct
    } else {
        Verdict::Incorrect
    };

    let step = session.apply_verdict(verdict, now);

    let mut tx = state.pool.begin().await.map_err(ApiError::from)?;

    // Record attempt
    let attempt_id = Uuid::new_v4().to_string();
    let verdict_str = serde_json::to_string(&verdict).unwrap().trim_matches('"').to_string();
    sqlx::query(
        r#"
        INSERT INTO attempts (
            id, session_id, student_id, question_id, try_no, input_kind,
            input_text, verdict, graded_by, created_at
        ) VALUES (?, ?, ?, ?, ?, 'self', ?, ?, 'self', ?)
        "#,
    )
    .bind(&attempt_id)
    .bind(&session.id)
    .bind(session.student_id)
    .bind(&q.id)
    .bind(session.q_try)
    .bind(if payload.correct { "自評正確" } else { "自評錯誤" })
    .bind(&verdict_str)
    .bind(now.to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::from)?;

    save_session(&mut *tx, &session).await.map_err(ApiError::from)?;

    if let Some(outcome) = step.outcome {
        let prev_m = load_progress(&mut *tx, session.student_id, &q.id).await.map_err(ApiError::from)?;
        let next_m = next_mastery(prev_m, session.student_id, &q.id, outcome, now);
        save_progress(&mut *tx, &next_m).await.map_err(ApiError::from)?;
    }

    tx.commit().await.map_err(ApiError::from)?;

    let say = if payload.correct {
        "太棒了！自己對答案也做得很誠實喔！".to_string()
    } else {
        "沒關係，再練習一次就會記住了！".to_string()
    };

    let reveal = if step.reveal {
        Some(RevealInfo {
            answer: answer_display(&q),
            explanation: q.explanation.clone(),
        })
    } else {
        None
    };

    Ok(Json(TutorReply {
        verdict: verdict_str,
        say,
        action: serde_json::to_string(&step.action).unwrap().trim_matches('"').to_string(),
        book_ref: None,
        recognized_text: None,
        followup_question: None,
        q_state: serde_json::to_string(&session.q_state).unwrap().trim_matches('"').to_string(),
        reveal,
        can_next: session.can_next(),
        self_grade_available: Some(false),
    }))
}

async fn build_prompt_context(
    conn: &mut sqlx::SqliteConnection,
    session: &Session,
    q: &Question,
    choice: Option<String>,
    msg: Option<String>,
) -> Result<PromptContext, ApiError> {
    let student_row: (String, i64) = sqlx::query_as("SELECT name, grade FROM students WHERE id = ?")
        .bind(session.student_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(ApiError::from)?;

    let sub_row: Option<(String, String)> = sqlx::query_as("SELECT name, book_title FROM subjects WHERE id = ?")
        .bind(&session.subject_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(ApiError::from)?;

    let (subject_name, book_title) = sub_row.unwrap_or_else(|| (session.subject_id.clone(), "課本".into()));

    let dialog_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM dialog_turns WHERE session_id = ? AND question_id = ? ORDER BY id ASC LIMIT 10"
    )
    .bind(&session.id)
    .bind(&q.id)
    .fetch_all(&mut *conn)
    .await
    .map_err(ApiError::from)?;

    let dialog = dialog_rows
        .into_iter()
        .map(|(role, content)| PromptDialogItem { role, content })
        .collect();

    let weak_items = load_weaknesses(&mut *conn, session.student_id, 5)
        .await
        .map_err(ApiError::from)?;

    let weak_concepts = weak_items
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
        try_no: session.q_try + 1,
        dialog,
        weak_concepts,
        student_choice: choice,
        student_message: msg,
        q_state: session.q_state,
    })
}

fn base64_simd_or_standard_decode(s: &str) -> Result<Vec<u8>, String> {
    // Standard base64 decoding helper without extra dependencies
    let mut table = [0u8; 256];
    for (i, &b) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".iter().enumerate() {
        table[b as usize] = i as u8;
    }

    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.len() % 4 != 0 {
        return Err("Invalid base64 length".into());
    }

    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let b0 = table[chunk[0] as usize];
        let b1 = table[chunk[1] as usize];
        let b2 = table[chunk[2] as usize];
        let b3 = table[chunk[3] as usize];

        let triple = ((b0 as u32) << 18) | ((b1 as u32) << 12) | ((b2 as u32) << 6) | (b3 as u32);
        out.push(((triple >> 16) & 0xFF) as u8);
        if chunk[2] != b'=' {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            out.push((triple & 0xFF) as u8);
        }
    }
    Ok(out)
}

use std::time::Duration;
use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use base64::Engine as _;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use ai_cli::{AiPurpose, RunFile};
use crate::tutor::ai::build_prompt_context;
use crate::tutor::ai::prompts::build_context_json;
use crate::tutor::ai::turn::{call_turn, canned_praise, canned_wrong_reply, TutorFeedback};
use crate::tutor::domain::question::{describe_choice, grade_choice, ChoiceInput, QType, Question, Verdict};
use crate::tutor::domain::session::{QState, Session, TeachAction};
use crate::tutor::shared::dto::{RevealInfo, TutorReply};
use crate::tutor::shared::enum_tag;
use crate::tutor::shared::error::ApiError;
use crate::tutor::store::{
    insert_attempt, insert_dialog_turn, load_question, load_session, record_outcome, save_session,
    NewAttempt,
};
use crate::tutor::TutorState;

const MAX_IMAGE_BYTES: usize = 3 * 1024 * 1024;
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const AI_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Deserialize)]
pub struct AnswerReq {
    pub question_id: String,
    pub kind: String, // 'choice' | 'text' | 'handwriting'
    pub choice: Option<serde_json::Value>, // int or string
    pub text: Option<String>,
    pub image_png_base64: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HintReq {
    pub question_id: String,
    /// 作答回覆的 action：hint | explain
    pub action: String,
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
        .route("/sessions/{id}/hint", post(handle_hint))
}

/// 同一個 session 同時只處理一個作答請求；drop 時自動解鎖
struct SessionLock {
    sid: String,
    locks: std::sync::Arc<dashmap::DashMap<String, ()>>,
}

impl SessionLock {
    fn acquire(state: &TutorState, sid: &str) -> Result<Self, ApiError> {
        if state.session_locks.insert(sid.to_string(), ()).is_some() {
            return Err(ApiError::Conflict("Another request is processing for this session".into()));
        }
        Ok(Self {
            sid: sid.to_string(),
            locks: state.session_locks.clone(),
        })
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        self.locks.remove(&self.sid);
    }
}

/// 載入 session 與目前題目，並確認題目仍在作答中
async fn load_answerable(
    conn: &mut sqlx::SqliteConnection,
    sid: &str,
    question_id: &str,
) -> Result<(Session, Question), ApiError> {
    let session = load_session(&mut *conn, sid)
        .await?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    if session.current_question_id() != Some(question_id) {
        return Err(ApiError::BadRequest("Question ID does not match current question".into()));
    }
    if !matches!(session.q_state, QState::Asking | QState::Hinting) {
        return Err(ApiError::Conflict("Current question is already completed".into()));
    }

    let q = load_question(&mut *conn, question_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;
    Ok((session, q))
}

/// 批改結果：規則批改沒有 AI 回饋
struct Graded {
    verdict: Verdict,
    graded_by: &'static str,
    feedback: Option<TutorFeedback>,
    choice_desc: Option<String>,
}

async fn handle_answer(
    State(state): State<TutorState>,
    Path(id): Path<String>,
    Json(payload): Json<AnswerReq>,
) -> Result<Json<TutorReply>, ApiError> {
    let _lock = SessionLock::acquire(&state, &id)?;
    let mut conn = state.pool.acquire().await?;
    let (mut session, q) = load_answerable(&mut conn, &id, &payload.question_id).await?;

    let now = Utc::now();
    let attempt_id = Uuid::new_v4().to_string();

    let handwriting = if payload.kind == "handwriting" {
        let b64 = payload
            .image_png_base64
            .as_deref()
            .ok_or_else(|| ApiError::BadRequest("Missing image for handwriting".into()))?;
        Some(save_handwriting_png(&state, &attempt_id, b64).await?)
    } else {
        None
    };

    // 1. 批改：是非/選擇題用規則，其餘交給 vision AI
    let is_rule_gradable = payload.kind == "choice"
        || (payload.kind == "text" && matches!(q.qtype, QType::TrueFalse | QType::SingleChoice));

    let graded = if is_rule_gradable {
        let input = choice_input(&payload);
        Graded {
            verdict: grade_choice(&q, &input),
            graded_by: "rule",
            feedback: None,
            choice_desc: Some(describe_choice(&q, &input)),
        }
    } else {
        let engines = state.ai.engines_for(AiPurpose::GradeHandwriting).await;
        if engines.is_empty() {
            // 沒有 vision 引擎 → 讓孩子看標準答案自己對
            return Ok(Json(TutorReply {
                verdict: "none".into(),
                say: "目前 AI 辨識尚未連線，請點選標準答案自己對答案喔！".into(),
                action: "self_grade".into(),
                book_ref: None,
                recognized_text: None,
                followup_question: None,
                q_state: enum_tag(&session.q_state),
                reveal: Some(RevealInfo::for_question(&q)),
                can_next: false,
                self_grade_available: Some(true),
                hint_pending: None,
            }));
        }
        let image = handwriting.as_ref().map(|(_, bytes)| bytes.clone());
        let fb = grade_with_ai(&state, &mut conn, &session, &q, &engines, image).await?;
        Graded {
            verdict: fb.verdict,
            graded_by: "ai",
            feedback: Some(fb),
            choice_desc: None,
        }
    };

    // 2. 狀態轉移由 domain 決定
    let step = session.apply_verdict(graded.verdict, now);

    // 3. 家教回覆（規則批改答錯時先回罐頭語，AI 提示由 /hint 另外取，避免孩子乾等）
    let action = enum_tag(&step.action);
    let mut book_ref = None;
    let mut followup_q = None;
    let mut hint_pending = false;
    let say = match (&graded.feedback, step.action) {
        (Some(fb), action) => {
            if action != TeachAction::Praise {
                book_ref = fb.book_ref.clone();
            }
            followup_q = fb.followup_question.clone();
            fb.say.clone()
        }
        (None, TeachAction::Praise) => canned_praise().to_string(),
        (None, TeachAction::Hint | TeachAction::Explain) => {
            let (say, br) = canned_wrong_reply(&q, step.action);
            book_ref = br;
            hint_pending = !state.ai.engines_for(AiPurpose::TutorWrong).await.is_empty();
            say
        }
        (None, TeachAction::RetryUnreadable) => {
            "字跡有點不清楚，請寫大一點、慢一點再試一次喔！".to_string()
        }
    };

    // 4. 單一交易寫入
    let verdict = enum_tag(&graded.verdict);
    let recognized_text = graded.feedback.as_ref().map(|f| f.recognized_text.clone());
    let input_text = graded.choice_desc.or(payload.text);
    let mut tx = state.pool.begin().await?;

    insert_attempt(
        &mut tx,
        &NewAttempt {
            id: &attempt_id,
            session: &session,
            question_id: &q.id,
            input_kind: &payload.kind,
            input_text: input_text.as_deref(),
            input_image_path: handwriting.as_ref().map(|(path, _)| path.as_str()),
            recognized_text: recognized_text.as_deref(),
            verdict: &verdict,
            graded_by: graded.graded_by,
            created_at: now,
        },
    )
    .await?;

    // 提示待 AI 補上時，對話由 /hint 寫入
    if !hint_pending {
        insert_dialog_turn(&mut tx, &session.id, &q.id, "tutor", &say, &action, book_ref.as_ref(), now)
            .await?;
    }

    save_session(&mut tx, &session).await?;

    if let Some(outcome) = step.outcome {
        let note = graded.feedback.as_ref().and_then(|f| f.misconception.clone());
        record_outcome(&mut tx, session.student_id, &q, outcome, note, now).await?;
    }

    tx.commit().await?;

    Ok(Json(TutorReply {
        verdict,
        say,
        action,
        book_ref,
        recognized_text,
        followup_question: followup_q,
        q_state: enum_tag(&session.q_state),
        reveal: step.reveal.then(|| RevealInfo::for_question(&q)),
        can_next: session.can_next(),
        self_grade_available: Some(false),
        hint_pending: hint_pending.then_some(true),
    }))
}

/// API：是非題送 true/false，選擇題送 1-based 題號，也接受文字
fn choice_input(payload: &AnswerReq) -> ChoiceInput<'_> {
    match &payload.choice {
        Some(serde_json::Value::Bool(b)) => ChoiceInput::Bool(*b),
        Some(serde_json::Value::Number(n)) => {
            ChoiceInput::Index((n.as_i64().unwrap_or(1) - 1).max(0) as usize)
        }
        Some(serde_json::Value::String(s)) => ChoiceInput::Text(s),
        _ => ChoiceInput::Text(payload.text.as_deref().unwrap_or("")),
    }
}

/// 驗證並存下手寫 PNG，回傳（相對 data_dir 的路徑, 圖檔位元組）
async fn save_handwriting_png(
    state: &TutorState,
    attempt_id: &str,
    b64: &str,
) -> Result<(String, Vec<u8>), ApiError> {
    let clean: String = b64
        .trim()
        .trim_start_matches("data:image/png;base64,")
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(clean)
        .map_err(|e| ApiError::BadRequest(format!("Invalid base64 image: {e}")))?;

    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ApiError::BadRequest("Image exceeds 3MB limit".into()));
    }
    if !bytes.starts_with(PNG_MAGIC) {
        return Err(ApiError::BadRequest("Image must be valid PNG format".into()));
    }

    let file_rel = format!("answers/{attempt_id}.png");
    let file_abs = state.data_dir.join(&file_rel);
    if let Some(parent) = file_abs.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::write(&file_abs, &bytes)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to save answer image: {e}")))?;

    Ok((file_rel, bytes))
}

/// 手寫／簡答題交給 vision AI 批改；題目有插圖時一併附上
async fn grade_with_ai(
    state: &TutorState,
    conn: &mut sqlx::SqliteConnection,
    session: &Session,
    q: &Question,
    engines: &[std::sync::Arc<dyn ai_cli::AiEngine>],
    answer_png: Option<Vec<u8>>,
) -> Result<TutorFeedback, ApiError> {
    let ctx = build_prompt_context(conn, session, q, session.q_try + 1, None, None).await?;
    let mut run_files = vec![RunFile {
        name: "context.json".into(),
        bytes: build_context_json(&ctx).into_bytes(),
    }];
    let mut images = Vec::new();

    if let Some(bytes) = answer_png {
        run_files.push(RunFile { name: "answer.png".into(), bytes });
        images.push("answer.png".into());
    }

    if let Some(ref img_path) = q.image_path {
        let full_p = state.data_dir.join("assets").join(img_path);
        if let Ok(bytes) = tokio::fs::read(&full_p).await {
            run_files.push(RunFile { name: "question.png".into(), bytes });
            images.push("question.png".into());
        }
    }

    let _permit = state.ai_sem.acquire().await.map_err(|e| ApiError::Internal(e.to_string()))?;
    call_turn(
        engines,
        AiPurpose::GradeHandwriting,
        &ctx,
        run_files,
        images,
        &state.pool,
        Some(&session.id),
        AI_TIMEOUT,
    )
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))
}

async fn handle_self_grade(
    State(state): State<TutorState>,
    Path(id): Path<String>,
    Json(payload): Json<SelfGradeReq>,
) -> Result<Json<TutorReply>, ApiError> {
    let _lock = SessionLock::acquire(&state, &id)?;
    let mut conn = state.pool.acquire().await?;
    let (mut session, q) = load_answerable(&mut conn, &id, &payload.question_id).await?;

    let now = Utc::now();
    let verdict = if payload.correct { Verdict::Correct } else { Verdict::Incorrect };
    let step = session.apply_verdict(verdict, now);
    let verdict_str = enum_tag(&verdict);

    let mut tx = state.pool.begin().await?;
    insert_attempt(
        &mut tx,
        &NewAttempt {
            id: &Uuid::new_v4().to_string(),
            session: &session,
            question_id: &q.id,
            input_kind: "self",
            input_text: Some(if payload.correct { "自評正確" } else { "自評錯誤" }),
            input_image_path: None,
            recognized_text: None,
            verdict: &verdict_str,
            graded_by: "self",
            created_at: now,
        },
    )
    .await?;
    save_session(&mut tx, &session).await?;
    if let Some(outcome) = step.outcome {
        record_outcome(&mut tx, session.student_id, &q, outcome, None, now).await?;
    }
    tx.commit().await?;

    let say = if payload.correct {
        "太棒了！自己對答案也做得很誠實喔！"
    } else {
        "沒關係，再練習一次就會記住了！"
    };

    Ok(Json(TutorReply {
        verdict: verdict_str,
        say: say.to_string(),
        action: enum_tag(&step.action),
        book_ref: None,
        recognized_text: None,
        followup_question: None,
        q_state: enum_tag(&session.q_state),
        reveal: step.reveal.then(|| RevealInfo::for_question(&q)),
        can_next: session.can_next(),
        self_grade_available: Some(false),
        hint_pending: None,
    }))
}

/// 規則批改答錯後的 AI 提示；AI 失敗或沒有引擎時回罐頭提示，一定寫入對話紀錄
async fn handle_hint(
    State(state): State<TutorState>,
    Path(id): Path<String>,
    Json(payload): Json<HintReq>,
) -> Result<Json<TutorReply>, ApiError> {
    let action = match payload.action.as_str() {
        "hint" => TeachAction::Hint,
        "explain" => TeachAction::Explain,
        _ => return Err(ApiError::BadRequest("action must be hint or explain".into())),
    };

    let mut conn = state.pool.acquire().await?;
    let session = load_session(&mut *conn, &id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Session not found".into()))?;

    let q = load_question(&mut *conn, &payload.question_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Question not found".into()))?;

    let last_input: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT input_text FROM attempts WHERE session_id = ? AND question_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&session.id)
    .bind(&q.id)
    .fetch_optional(&mut *conn)
    .await?;
    let choice_str = last_input.and_then(|(t,)| t);

    let (mut say, mut book_ref) = canned_wrong_reply(&q, action);
    let mut followup_q = None;

    let engines = state.ai.engines_for(AiPurpose::TutorWrong).await;
    if !engines.is_empty() {
        let ctx = build_prompt_context(&mut conn, &session, &q, session.q_try + 1, choice_str, None).await?;
        let run_files = vec![RunFile {
            name: "context.json".into(),
            bytes: build_context_json(&ctx).into_bytes(),
        }];
        let _permit = state.ai_sem.acquire().await.map_err(|e| ApiError::Internal(e.to_string()))?;
        match call_turn(
            &engines,
            AiPurpose::TutorWrong,
            &ctx,
            run_files,
            vec![],
            &state.pool,
            Some(&session.id),
            AI_TIMEOUT,
        )
        .await
        {
            Ok(fb) if !fb.say.trim().is_empty() => {
                say = fb.say;
                book_ref = fb.book_ref.or(book_ref);
                followup_q = fb.followup_question;
            }
            Ok(_) => {}
            Err(e) => eprintln!("[tutor] hint AI failed, using canned reply: {e}"),
        }
    }

    let action_str = enum_tag(&action);
    insert_dialog_turn(&mut conn, &session.id, &q.id, "tutor", &say, &action_str, book_ref.as_ref(), Utc::now())
        .await?;

    Ok(Json(TutorReply {
        verdict: "incorrect".into(),
        say,
        action: action_str,
        book_ref,
        recognized_text: None,
        followup_question: followup_q,
        q_state: enum_tag(&session.q_state),
        reveal: None,
        can_next: session.can_next(),
        self_grade_available: None,
        hint_pending: None,
    }))
}

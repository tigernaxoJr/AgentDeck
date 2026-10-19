//! HTTP 端到端測試：真實的 router + 暫存 SQLite 檔 + MockEngine，不呼叫任何 CLI。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ai_cli::mock::MockEngine;
use ai_cli::{AiEngine, AiRawResult};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use dashmap::DashMap;
use serde_json::{json, Value};
use tokio::sync::{RwLock, Semaphore};
use tower::ServiceExt;

use crate::tutor::ai::Backends;
use crate::tutor::domain::question::{Answer, InputMode, QType, Question};
use crate::tutor::store::save_question;
use crate::tutor::{routes, TutorState};

const API_KEY: &str = "test-key";

struct TestApp {
    router: Router,
    pool: sqlx::SqlitePool,
    data_dir: PathBuf,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

fn question(id: &str, ord: i64, qtype: QType, options: &[&str], answer: Answer) -> Question {
    Question {
        id: id.into(),
        subject_id: "social".into(),
        unit_id: "social_01".into(),
        ord,
        section: "".into(),
        qtype,
        stem: format!("題目 {id}"),
        options: options.iter().map(|s| s.to_string()).collect(),
        answer,
        explanation: "因為地圖上方是北方".into(),
        keywords: vec![],
        char_count: None,
        image_path: None,
        book_page: Some(12),
        book_where: Some("上方的地圖".into()),
        concept_tags: vec!["方位".into()],
        input_mode: InputMode::Choice,
        source: "test".into(),
        active: true,
        updated_at: "2026-10-19T00:00:00Z".into(),
    }
}

/// 單一回合的 mock AI 回覆（寫入 out/turn.json 的內容）
fn mock_turn(turn: Value) -> Result<AiRawResult, ai_cli::AiError> {
    Ok(AiRawResult {
        backend_id: "mock".into(),
        output_file: Some(turn.to_string()),
        final_text: None,
        usage: None,
        duration: Duration::from_millis(1),
        stderr_tail: String::new(),
    })
}

async fn test_app(engines: Vec<Arc<dyn AiEngine>>, vision: Vec<Arc<dyn AiEngine>>) -> TestApp {
    let data_dir = std::env::temp_dir().join(format!("tutor-test-{}", uuid::Uuid::new_v4()));
    let pool = crate::tutor::shared::db::open(&data_dir).await.expect("open test db");

    sqlx::raw_sql(
        r#"
        INSERT INTO students (id, name, grade, created_at) VALUES (1, '小明', 4, '2026-10-19T00:00:00Z');
        INSERT INTO subjects (id, name, book_title) VALUES ('social', '社會', '四上 社會');
        INSERT INTO units (id, subject_id, title, ord) VALUES ('social_01', 'social', '第 1 回', 1);
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed");

    let mut conn = pool.acquire().await.expect("acquire");
    let q1 = question("q_choice", 1, QType::SingleChoice, &["東", "西", "南", "北"], Answer::Choice(3));
    let q2 = question("q_tf", 2, QType::TrueFalse, &["O", "X"], Answer::TrueFalse(true));
    save_question(&mut conn, &q1).await.expect("save q1");
    save_question(&mut conn, &q2).await.expect("save q2");
    drop(conn);

    let ai = Arc::new(Backends {
        pool: pool.clone(),
        data_dir: data_dir.clone(),
        engines: RwLock::new(engines),
        vision_engines: RwLock::new(vision),
    });
    let state = TutorState {
        pool: pool.clone(),
        data_dir: data_dir.clone(),
        ai,
        ai_sem: Arc::new(Semaphore::new(2)),
        session_locks: Arc::new(DashMap::new()),
    };
    let router = Router::new().nest("/api/tutor", routes(state, Arc::new(API_KEY.to_string())));
    TestApp { router, pool, data_dir }
}

impl TestApp {
    async fn call(&self, method: &str, uri: &str, body: Option<Value>, key: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(k) = key {
            req = req.header("authorization", format!("Bearer {k}"));
        }
        let req = match body {
            Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .expect("build request");

        let res = self.router.clone().oneshot(req).await.expect("oneshot");
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.expect("read body");
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.call("POST", uri, Some(body), None).await
    }

    async fn start_session(&self) -> (String, String) {
        let (status, view) = self
            .post(
                "/api/tutor/sessions",
                json!({ "student_id": 1, "mode": "progress", "subject_id": "social", "unit_id": "social_01", "limit": 5 }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "start session: {view}");
        let sid = view["sessionId"].as_str().expect("session_id").to_string();
        let qid = view["question"]["id"].as_str().expect("question id").to_string();
        (sid, qid)
    }
}

/// 揭曉前任何回應都不能出現正解或解析
fn assert_no_leak(v: &Value) {
    let s = v.to_string();
    assert!(!s.contains("因為地圖上方是北方"), "explanation leaked: {s}");
    assert!(v.get("reveal").is_none(), "reveal leaked: {s}");
}

#[tokio::test]
async fn wrong_twice_hints_then_explains_and_requeues() {
    let hint = mock_turn(json!({
        "verdict": "incorrect",
        "say": "想想看，地圖的上方通常代表哪個方位？",
        "action": "hint",
        "book_ref": { "page": 12, "where": "上方的地圖" }
    }));
    let engine: Arc<dyn AiEngine> = Arc::new(MockEngine::new("mock", vec![hint]));
    let app = test_app(vec![engine], vec![]).await;

    let (sid, qid) = app.start_session().await;
    assert_eq!(qid, "q_choice");
    let (_, view) = app.call("GET", &format!("/api/tutor/sessions/{sid}"), None, None).await;
    assert_no_leak(&view);

    // 第一次答錯：立即回對錯，AI 提示待 /hint
    let (status, r1) = app
        .post(&format!("/api/tutor/sessions/{sid}/answer"), json!({ "question_id": qid, "kind": "choice", "choice": 1 }))
        .await;
    assert_eq!(status, StatusCode::OK, "{r1}");
    assert_eq!(r1["verdict"], "incorrect");
    assert_eq!(r1["action"], "hint");
    assert_eq!(r1["hintPending"], true);
    assert_eq!(r1["canNext"], false);
    assert_no_leak(&r1);

    let (status, h) = app
        .post(&format!("/api/tutor/sessions/{sid}/hint"), json!({ "question_id": qid, "action": "hint" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{h}");
    assert_eq!(h["say"], "想想看，地圖的上方通常代表哪個方位？");
    assert_no_leak(&h);

    // 第二次答錯：講解並揭曉，題目排回佇列尾端
    let (_, r2) = app
        .post(&format!("/api/tutor/sessions/{sid}/answer"), json!({ "question_id": qid, "kind": "choice", "choice": 2 }))
        .await;
    assert_eq!(r2["action"], "explain");
    assert_eq!(r2["canNext"], true);
    assert!(r2["reveal"].is_object(), "{r2}");

    // 已結束的題目不可再作答或自評
    let (status, _) = app
        .post(&format!("/api/tutor/sessions/{sid}/answer"), json!({ "question_id": qid, "kind": "choice", "choice": 3 }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app
        .post(&format!("/api/tutor/sessions/{sid}/self_grade"), json!({ "question_id": qid, "correct": true }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attempts WHERE session_id = ?")
        .bind(&sid)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(attempts, 2);
    let weakness: f64 = sqlx::query_scalar("SELECT score FROM concept_weakness WHERE student_id = 1 AND concept_tag = '方位'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(weakness > 0.0);
}

#[tokio::test]
async fn correct_answer_praises_and_saves_mastery_without_ai() {
    let app = test_app(vec![], vec![]).await;
    let (sid, qid) = app.start_session().await;

    let (status, r) = app
        .post(&format!("/api/tutor/sessions/{sid}/answer"), json!({ "question_id": qid, "kind": "choice", "choice": 4 }))
        .await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["verdict"], "correct");
    assert_eq!(r["action"], "praise");
    assert!(r.get("hintPending").is_none());

    let boxes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mastery WHERE student_id = 1 AND question_id = ?")
        .bind(&qid)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(boxes, 1);
}

#[tokio::test]
async fn wrong_answer_without_ai_engine_uses_canned_hint() {
    let app = test_app(vec![], vec![]).await;
    let (sid, qid) = app.start_session().await;

    let (_, r) = app
        .post(&format!("/api/tutor/sessions/{sid}/answer"), json!({ "question_id": qid, "kind": "choice", "choice": 1 }))
        .await;
    assert_eq!(r["action"], "hint");
    assert!(r.get("hintPending").is_none(), "{r}");
    assert_eq!(r["bookRef"]["page"], 12);
}

#[tokio::test]
async fn handwriting_rejects_bad_images() {
    let app = test_app(vec![], vec![]).await;
    let (sid, qid) = app.start_session().await;
    let uri = format!("/api/tutor/sessions/{sid}/answer");

    let (status, _) = app
        .post(&uri, json!({ "question_id": qid, "kind": "handwriting", "image_png_base64": "not base64!!" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 合法 base64 但不是 PNG
    let (status, _) = app
        .post(&uri, json!({ "question_id": qid, "kind": "handwriting", "image_png_base64": "aGVsbG8gd29ybGQ=" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = app.post(&uri, json!({ "question_id": qid, "kind": "handwriting" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_routes_require_api_key() {
    let app = test_app(vec![], vec![]).await;

    let (status, _) = app.call("GET", "/api/tutor/admin/ai/calls", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app.call("GET", "/api/tutor/admin/ai/calls", None, Some("wrong")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app.call("GET", "/api/tutor/admin/ai/calls", None, Some(API_KEY)).await;
    assert_eq!(status, StatusCode::OK);

    // 學童端不需要金鑰
    let (status, _) = app.call("GET", "/api/tutor/subjects", None, None).await;
    assert_eq!(status, StatusCode::OK);
}

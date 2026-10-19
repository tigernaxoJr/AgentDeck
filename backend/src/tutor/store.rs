use chrono::{DateTime, Utc};
use sqlx::{Row, SqliteConnection};

use crate::tutor::domain::progress::{ConceptWeakness, Mastery};
use crate::tutor::domain::question::{Answer, InputMode, QType, Question};
use crate::tutor::domain::session::{QState, Session};
use crate::tutor::shared::enum_tag;

pub async fn load_question(
    conn: &mut SqliteConnection,
    question_id: &str,
) -> Result<Option<Question>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, subject_id, unit_id, ord, section, qtype, stem, options_json,
               answer_json, explanation, keywords_json, char_count, image_path,
               book_page, book_where, concept_tags_json, input_mode, source,
               active, updated_at
        FROM questions
        WHERE id = ?
        "#,
    )
    .bind(question_id)
    .fetch_optional(conn)
    .await?;

    let row = match row {
        Some(r) => r,
        None => return Ok(None),
    };

    let qtype_str: String = row.get("qtype");
    let qtype: QType = serde_json::from_str(&format!("\"{qtype_str}\""))
        .unwrap_or(QType::ShortAnswer);

    let input_mode_str: String = row.get("input_mode");
    let input_mode: InputMode = serde_json::from_str(&format!("\"{input_mode_str}\""))
        .unwrap_or(InputMode::Text);

    let options_json: String = row.get("options_json");
    let options: Vec<String> = serde_json::from_str(&options_json).unwrap_or_default();

    let answer_json: String = row.get("answer_json");
    let answer: Answer = serde_json::from_str(&answer_json).unwrap_or(Answer::Text(String::new()));

    let keywords_json: String = row.get("keywords_json");
    let keywords: Vec<String> = serde_json::from_str(&keywords_json).unwrap_or_default();

    let concept_tags_json: String = row.get("concept_tags_json");
    let concept_tags: Vec<String> = serde_json::from_str(&concept_tags_json).unwrap_or_default();

    let active_int: i64 = row.get("active");

    Ok(Some(Question {
        id: row.get("id"),
        subject_id: row.get("subject_id"),
        unit_id: row.get("unit_id"),
        ord: row.get("ord"),
        section: row.get("section"),
        qtype,
        stem: row.get("stem"),
        options,
        answer,
        explanation: row.get("explanation"),
        keywords,
        char_count: row.get("char_count"),
        image_path: row.get("image_path"),
        book_page: row.get("book_page"),
        book_where: row.get("book_where"),
        concept_tags,
        input_mode,
        source: row.get("source"),
        active: active_int != 0,
        updated_at: row.get("updated_at"),
    }))
}

pub async fn save_question(
    conn: &mut SqliteConnection,
    q: &Question,
) -> Result<(), sqlx::Error> {
    let qtype_str = enum_tag(&q.qtype);
    let input_mode_str = enum_tag(&q.input_mode);
    let options_json = serde_json::to_string(&q.options).unwrap();
    let answer_json = serde_json::to_string(&q.answer).unwrap();
    let keywords_json = serde_json::to_string(&q.keywords).unwrap();
    let concept_tags_json = serde_json::to_string(&q.concept_tags).unwrap();
    let active_int = if q.active { 1 } else { 0 };

    sqlx::query(
        r#"
        INSERT INTO questions (
            id, subject_id, unit_id, ord, section, qtype, stem, options_json,
            answer_json, explanation, keywords_json, char_count, image_path,
            book_page, book_where, concept_tags_json, input_mode, source,
            active, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            stem = excluded.stem,
            options_json = excluded.options_json,
            answer_json = excluded.answer_json,
            explanation = excluded.explanation,
            keywords_json = excluded.keywords_json,
            char_count = excluded.char_count,
            image_path = excluded.image_path,
            book_page = COALESCE(questions.book_page, excluded.book_page),
            book_where = COALESCE(questions.book_where, excluded.book_where),
            concept_tags_json = CASE
                WHEN questions.concept_tags_json != '[]' THEN questions.concept_tags_json
                ELSE excluded.concept_tags_json
            END,
            input_mode = excluded.input_mode,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&q.id)
    .bind(&q.subject_id)
    .bind(&q.unit_id)
    .bind(q.ord)
    .bind(&q.section)
    .bind(qtype_str)
    .bind(&q.stem)
    .bind(options_json)
    .bind(answer_json)
    .bind(&q.explanation)
    .bind(keywords_json)
    .bind(q.char_count)
    .bind(&q.image_path)
    .bind(q.book_page)
    .bind(&q.book_where)
    .bind(concept_tags_json)
    .bind(input_mode_str)
    .bind(&q.source)
    .bind(active_int)
    .bind(&q.updated_at)
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn load_session(
    conn: &mut SqliteConnection,
    session_id: &str,
) -> Result<Option<Session>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, student_id, mode, subject_id, unit_id, queue_json, cursor,
               q_state, q_try, q_hinted, started_at, ended_at
        FROM sessions
        WHERE id = ?
        "#,
    )
    .bind(session_id)
    .fetch_optional(conn)
    .await?;

    let row = match row {
        Some(r) => r,
        None => return Ok(None),
    };

    let queue_json: String = row.get("queue_json");
    let queue: Vec<String> = serde_json::from_str(&queue_json).unwrap_or_default();

    let q_state_str: String = row.get("q_state");
    let q_state: QState = serde_json::from_str(&format!("\"{q_state_str}\""))
        .unwrap_or(QState::Asking);

    let started_at_str: String = row.get("started_at");
    let started_at = DateTime::parse_from_rfc3339(&started_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let ended_at_str: Option<String> = row.get("ended_at");
    let ended_at = ended_at_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    });

    let q_hinted_int: i64 = row.get("q_hinted");
    let cursor_i64: i64 = row.get("cursor");

    Ok(Some(Session {
        id: row.get("id"),
        student_id: row.get("student_id"),
        mode: row.get("mode"),
        subject_id: row.get("subject_id"),
        unit_id: row.get("unit_id"),
        queue,
        cursor: cursor_i64.max(0) as usize,
        q_state,
        q_try: row.get("q_try"),
        q_hinted: q_hinted_int != 0,
        started_at,
        ended_at,
    }))
}

pub async fn save_session(
    conn: &mut SqliteConnection,
    s: &Session,
) -> Result<(), sqlx::Error> {
    let queue_json = serde_json::to_string(&s.queue).unwrap();
    let q_state_str = enum_tag(&s.q_state);
    let q_hinted_int = if s.q_hinted { 1 } else { 0 };
    let started_at = s.started_at.to_rfc3339();
    let ended_at = s.ended_at.map(|dt| dt.to_rfc3339());

    sqlx::query(
        r#"
        INSERT INTO sessions (
            id, student_id, mode, subject_id, unit_id, queue_json, cursor,
            q_state, q_try, q_hinted, started_at, ended_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            queue_json = excluded.queue_json,
            cursor = excluded.cursor,
            q_state = excluded.q_state,
            q_try = excluded.q_try,
            q_hinted = excluded.q_hinted,
            ended_at = excluded.ended_at
        "#,
    )
    .bind(&s.id)
    .bind(s.student_id)
    .bind(&s.mode)
    .bind(&s.subject_id)
    .bind(&s.unit_id)
    .bind(queue_json)
    .bind(s.cursor as i64)
    .bind(q_state_str)
    .bind(s.q_try)
    .bind(q_hinted_int)
    .bind(started_at)
    .bind(ended_at)
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn load_progress(
    conn: &mut SqliteConnection,
    student_id: i64,
    question_id: &str,
) -> Result<Option<Mastery>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT student_id, question_id, box, due_at, streak, total_correct,
               total_wrong, last_verdict, last_seen_at
        FROM mastery
        WHERE student_id = ? AND question_id = ?
        "#,
    )
    .bind(student_id)
    .bind(question_id)
    .fetch_optional(conn)
    .await?;

    let row = match row {
        Some(r) => r,
        None => return Ok(None),
    };

    let due_at_str: String = row.get("due_at");
    let due_at = DateTime::parse_from_rfc3339(&due_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let last_seen_str: Option<String> = row.get("last_seen_at");
    let last_seen_at = last_seen_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    });

    Ok(Some(Mastery {
        student_id: row.get("student_id"),
        question_id: row.get("question_id"),
        box_level: row.get("box"),
        due_at,
        streak: row.get("streak"),
        total_correct: row.get("total_correct"),
        total_wrong: row.get("total_wrong"),
        last_verdict: row.get("last_verdict"),
        last_seen_at,
    }))
}

pub async fn save_progress(
    conn: &mut SqliteConnection,
    m: &Mastery,
) -> Result<(), sqlx::Error> {
    let due_at = m.due_at.to_rfc3339();
    let last_seen = m.last_seen_at.map(|dt| dt.to_rfc3339());

    sqlx::query(
        r#"
        INSERT INTO mastery (
            student_id, question_id, box, due_at, streak, total_correct,
            total_wrong, last_verdict, last_seen_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(student_id, question_id) DO UPDATE SET
            box = excluded.box,
            due_at = excluded.due_at,
            streak = excluded.streak,
            total_correct = excluded.total_correct,
            total_wrong = excluded.total_wrong,
            last_verdict = excluded.last_verdict,
            last_seen_at = excluded.last_seen_at
        "#,
    )
    .bind(m.student_id)
    .bind(&m.question_id)
    .bind(m.box_level)
    .bind(due_at)
    .bind(m.streak)
    .bind(m.total_correct)
    .bind(m.total_wrong)
    .bind(&m.last_verdict)
    .bind(last_seen)
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn load_weaknesses(
    conn: &mut SqliteConnection,
    student_id: i64,
    top: usize,
) -> Result<Vec<ConceptWeakness>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT student_id, concept_tag, score, wrong_count, last_note, updated_at
        FROM concept_weakness
        WHERE student_id = ? AND score > 0
        ORDER BY score DESC
        LIMIT ?
        "#,
    )
    .bind(student_id)
    .bind(top as i64)
    .fetch_all(conn)
    .await?;

    let mut result = Vec::new();
    for r in rows {
        let updated_at_str: String = r.get("updated_at");
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        result.push(ConceptWeakness {
            student_id: r.get("student_id"),
            concept_tag: r.get("concept_tag"),
            score: r.get("score"),
            wrong_count: r.get("wrong_count"),
            last_note: r.get("last_note"),
            updated_at,
        });
    }

    Ok(result)
}

pub async fn save_weakness(
    conn: &mut SqliteConnection,
    w: &ConceptWeakness,
) -> Result<(), sqlx::Error> {
    let updated_at = w.updated_at.to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO concept_weakness (
            student_id, concept_tag, score, wrong_count, last_note, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(student_id, concept_tag) DO UPDATE SET
            score = excluded.score,
            wrong_count = excluded.wrong_count,
            last_note = excluded.last_note,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(w.student_id)
    .bind(&w.concept_tag)
    .bind(w.score)
    .bind(w.wrong_count)
    .bind(&w.last_note)
    .bind(updated_at)
    .execute(conn)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tutor::shared::db::open_url;

    #[tokio::test]
    async fn test_store_roundtrip() {
        let pool = open_url("sqlite::memory:").await.expect("open memory db");
        let mut conn = pool.acquire().await.expect("acquire connection");

        // Prepare foreign keys
        sqlx::query("INSERT INTO students (id, name, created_at) VALUES (1, '小明', '2026-10-15T00:00:00Z')")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO subjects (id, name) VALUES ('social', '社會')")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO units (id, subject_id, title, ord) VALUES ('social_01', 'social', '單元一', 1)")
            .execute(&mut *conn)
            .await
            .unwrap();

        // 1. Question roundtrip
        let q = Question {
            id: "q1".into(),
            subject_id: "social".into(),
            unit_id: "social_01".into(),
            ord: 1,
            section: "sec1".into(),
            qtype: QType::SingleChoice,
            stem: "測試題目".into(),
            options: vec!["A".into(), "B".into()],
            answer: Answer::Choice(0),
            explanation: "解說".into(),
            keywords: vec!["關鍵詞".into()],
            char_count: Some(2),
            image_path: Some("social/crops/pic.png".into()),
            book_page: Some(5),
            book_where: Some("右上角".into()),
            concept_tags: vec!["方位".into()],
            input_mode: InputMode::Choice,
            source: "import".into(),
            active: true,
            updated_at: "2026-10-15T00:00:00Z".into(),
        };
        save_question(&mut *conn, &q).await.unwrap();
        let loaded_q = load_question(&mut *conn, "q1").await.unwrap().unwrap();
        assert_eq!(q, loaded_q);

        // 2. Session roundtrip
        let now = Utc::now();
        let s = Session {
            id: "sess1".into(),
            student_id: 1,
            mode: "progress".into(),
            subject_id: "social".into(),
            unit_id: Some("social_01".into()),
            queue: vec!["q1".into()],
            cursor: 0,
            q_state: QState::Asking,
            q_try: 1,
            q_hinted: true,
            started_at: now,
            ended_at: None,
        };
        save_session(&mut *conn, &s).await.unwrap();
        let loaded_s = load_session(&mut *conn, "sess1").await.unwrap().unwrap();
        assert_eq!(s.id, loaded_s.id);
        assert_eq!(s.student_id, loaded_s.student_id);
        assert_eq!(s.queue, loaded_s.queue);
        assert_eq!(s.q_state, loaded_s.q_state);
        assert_eq!(s.q_hinted, loaded_s.q_hinted);

        // 3. Progress roundtrip
        let m = Mastery {
            student_id: 1,
            question_id: "q1".into(),
            box_level: 2,
            due_at: now,
            streak: 3,
            total_correct: 4,
            total_wrong: 1,
            last_verdict: Some("correct".into()),
            last_seen_at: Some(now),
        };
        save_progress(&mut *conn, &m).await.unwrap();
        let loaded_m = load_progress(&mut *conn, 1, "q1").await.unwrap().unwrap();
        assert_eq!(m.student_id, loaded_m.student_id);
        assert_eq!(m.box_level, loaded_m.box_level);
        assert_eq!(m.streak, loaded_m.streak);

        // 4. Weakness roundtrip
        let w = ConceptWeakness {
            student_id: 1,
            concept_tag: "方位".into(),
            score: 2.5,
            wrong_count: 3,
            last_note: Some("筆誤".into()),
            updated_at: now,
        };
        save_weakness(&mut *conn, &w).await.unwrap();
        let list = load_weaknesses(&mut *conn, 1, 10).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].concept_tag, "方位");
        assert_eq!(list[0].score, 2.5);
    }
}

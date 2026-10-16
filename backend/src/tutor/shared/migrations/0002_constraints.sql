-- v2：補上外鍵、CHECK 約束、查詢用索引，並新增 question_concepts 關聯表。
-- SQLite 無法對既有欄位加約束，故依官方流程重建資料表：
-- 建新表 → 複製資料 → 刪舊表 → 改名。遷移器會在交易外關閉 foreign_keys，提交前執行 foreign_key_check。

-- ── 資料清理 ────────────────────────────────────────────────
-- 已停用、單元已不存在、且沒有任何作答紀錄的舊題目
DELETE FROM questions
WHERE unit_id NOT IN (SELECT id FROM units)
  AND active = 0
  AND id NOT IN (SELECT question_id FROM attempts)
  AND id NOT IN (SELECT question_id FROM mastery)
  AND id NOT IN (SELECT question_id FROM dialog_turns);

-- 早期前端把單元標題當成 unit_id 存入
UPDATE sessions SET unit_id = NULL
WHERE unit_id IS NOT NULL AND unit_id NOT IN (SELECT id FROM units);

-- ── students ────────────────────────────────────────────────
CREATE TABLE students_new (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL CHECK (trim(name) <> ''),
  grade       INTEGER NOT NULL DEFAULT 4 CHECK (grade BETWEEN 1 AND 12),
  created_at  TEXT NOT NULL
);
INSERT INTO students_new (id, name, grade, created_at)
  SELECT id, name, grade, created_at FROM students;
DROP TABLE students;
ALTER TABLE students_new RENAME TO students;

-- ── units ───────────────────────────────────────────────────
-- (id, subject_id) 唯一索引讓 questions 可用複合外鍵保證題目科目與單元科目一致
CREATE UNIQUE INDEX IF NOT EXISTS ux_units_id_subject ON units(id, subject_id);
CREATE INDEX IF NOT EXISTS idx_units_subject ON units(subject_id, ord);

-- ── questions ───────────────────────────────────────────────
CREATE TABLE questions_new (
  id            TEXT PRIMARY KEY,
  subject_id    TEXT NOT NULL REFERENCES subjects(id),
  unit_id       TEXT NOT NULL,
  ord           INTEGER NOT NULL,
  section       TEXT NOT NULL DEFAULT '',
  qtype         TEXT NOT NULL CHECK (qtype IN (
                  'true_false', 'single_choice', 'fill_blank', 'short_answer',
                  'math_expr', 'chinese_write', 'handwrite_blank')),
  stem          TEXT NOT NULL,
  options_json  TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(options_json)),
  answer_json   TEXT NOT NULL CHECK (json_valid(answer_json)),
  explanation   TEXT NOT NULL DEFAULT '',
  keywords_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(keywords_json)),
  char_count    INTEGER,
  image_path    TEXT,
  book_page     INTEGER,
  book_where    TEXT,
  concept_tags_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(concept_tags_json)),
  input_mode    TEXT NOT NULL CHECK (input_mode IN (
                  'choice', 'char_grid', 'free_canvas', 'image_canvas', 'text')),
  source        TEXT NOT NULL DEFAULT 'import',
  active        INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
  updated_at    TEXT NOT NULL,
  FOREIGN KEY (unit_id, subject_id) REFERENCES units(id, subject_id)
);
INSERT INTO questions_new (
  id, subject_id, unit_id, ord, section, qtype, stem, options_json, answer_json,
  explanation, keywords_json, char_count, image_path, book_page, book_where,
  concept_tags_json, input_mode, source, active, updated_at)
SELECT
  id, subject_id, unit_id, ord, section, qtype, stem, options_json, answer_json,
  explanation, keywords_json, char_count, image_path, book_page, book_where,
  concept_tags_json, input_mode, source, active, updated_at
FROM questions;
DROP TABLE questions;
ALTER TABLE questions_new RENAME TO questions;
CREATE INDEX idx_q_unit ON questions(unit_id, ord);
CREATE INDEX idx_q_subject_active ON questions(subject_id, active, ord);

-- ── question_concepts（由 concept_tags_json 衍生，觸發器自動同步）─────
CREATE TABLE question_concepts (
  question_id  TEXT NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
  concept_tag  TEXT NOT NULL,
  PRIMARY KEY (question_id, concept_tag)
) WITHOUT ROWID;
CREATE INDEX idx_qc_tag ON question_concepts(concept_tag, question_id);

INSERT OR IGNORE INTO question_concepts (question_id, concept_tag)
  SELECT q.id, j.value
  FROM questions q, json_each(q.concept_tags_json) j
  WHERE j.type = 'text' AND trim(j.value) <> '';

CREATE TRIGGER trg_questions_concepts_insert AFTER INSERT ON questions
BEGIN
  INSERT OR IGNORE INTO question_concepts (question_id, concept_tag)
    SELECT NEW.id, j.value FROM json_each(NEW.concept_tags_json) j
    WHERE j.type = 'text' AND trim(j.value) <> '';
END;

CREATE TRIGGER trg_questions_concepts_update AFTER UPDATE OF concept_tags_json ON questions
BEGIN
  DELETE FROM question_concepts WHERE question_id = NEW.id;
  INSERT OR IGNORE INTO question_concepts (question_id, concept_tag)
    SELECT NEW.id, j.value FROM json_each(NEW.concept_tags_json) j
    WHERE j.type = 'text' AND trim(j.value) <> '';
END;

-- ── sessions ────────────────────────────────────────────────
CREATE TABLE sessions_new (
  id          TEXT PRIMARY KEY,
  student_id  INTEGER NOT NULL REFERENCES students(id),
  mode        TEXT NOT NULL CHECK (mode IN ('progress', 'review', 'mixed')),
  subject_id  TEXT NOT NULL REFERENCES subjects(id),
  unit_id     TEXT REFERENCES units(id),
  queue_json  TEXT NOT NULL CHECK (json_valid(queue_json)),
  cursor      INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
  q_state     TEXT NOT NULL DEFAULT 'asking'
                CHECK (q_state IN ('asking', 'hinting', 'revealed', 'explained')),
  q_try       INTEGER NOT NULL DEFAULT 0 CHECK (q_try >= 0),
  q_hinted    INTEGER NOT NULL DEFAULT 0 CHECK (q_hinted IN (0, 1)),
  started_at  TEXT NOT NULL,
  ended_at    TEXT
);
INSERT INTO sessions_new (
  id, student_id, mode, subject_id, unit_id, queue_json, cursor,
  q_state, q_try, q_hinted, started_at, ended_at)
SELECT
  id, student_id, mode, subject_id, unit_id, queue_json, cursor,
  q_state, q_try, q_hinted, started_at, ended_at
FROM sessions;
DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;
CREATE INDEX idx_sessions_student ON sessions(student_id, started_at);

-- ── attempts ────────────────────────────────────────────────
CREATE TABLE attempts_new (
  id               TEXT PRIMARY KEY,
  session_id       TEXT NOT NULL REFERENCES sessions(id),
  student_id       INTEGER NOT NULL REFERENCES students(id),
  question_id      TEXT NOT NULL REFERENCES questions(id),
  try_no           INTEGER NOT NULL CHECK (try_no >= 0),
  input_kind       TEXT NOT NULL CHECK (input_kind IN ('choice', 'text', 'handwriting', 'self')),
  input_text       TEXT,
  input_image_path TEXT,
  recognized_text  TEXT,
  verdict          TEXT NOT NULL
                     CHECK (verdict IN ('correct', 'partial', 'incorrect', 'unreadable')),
  graded_by        TEXT NOT NULL CHECK (graded_by IN ('rule', 'ai', 'self')),
  created_at       TEXT NOT NULL
);
INSERT INTO attempts_new (
  id, session_id, student_id, question_id, try_no, input_kind, input_text,
  input_image_path, recognized_text, verdict, graded_by, created_at)
SELECT
  id, session_id, student_id, question_id, try_no, input_kind, input_text,
  input_image_path, recognized_text, verdict, graded_by, created_at
FROM attempts;
DROP TABLE attempts;
ALTER TABLE attempts_new RENAME TO attempts;
CREATE INDEX idx_att_student_q ON attempts(student_id, question_id);
CREATE INDEX idx_att_session_q ON attempts(session_id, question_id, created_at);
CREATE INDEX idx_att_question ON attempts(question_id);

-- ── dialog_turns ────────────────────────────────────────────
CREATE TABLE dialog_turns_new (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id    TEXT NOT NULL REFERENCES sessions(id),
  question_id   TEXT NOT NULL REFERENCES questions(id),
  role          TEXT NOT NULL CHECK (role IN ('tutor', 'student')),
  content       TEXT NOT NULL,
  action        TEXT,
  book_ref_json TEXT CHECK (book_ref_json IS NULL OR json_valid(book_ref_json)),
  created_at    TEXT NOT NULL
);
INSERT INTO dialog_turns_new (
  id, session_id, question_id, role, content, action, book_ref_json, created_at)
SELECT id, session_id, question_id, role, content, action, book_ref_json, created_at
FROM dialog_turns;
DROP TABLE dialog_turns;
ALTER TABLE dialog_turns_new RENAME TO dialog_turns;
CREATE INDEX idx_dlg ON dialog_turns(session_id, question_id, id);
CREATE INDEX idx_dlg_question ON dialog_turns(question_id);

-- ── mastery ─────────────────────────────────────────────────
CREATE TABLE mastery_new (
  student_id     INTEGER NOT NULL REFERENCES students(id),
  question_id    TEXT NOT NULL REFERENCES questions(id),
  box            INTEGER NOT NULL DEFAULT 0 CHECK (box BETWEEN 0 AND 5),
  due_at         TEXT NOT NULL,
  streak         INTEGER NOT NULL DEFAULT 0 CHECK (streak >= 0),
  total_correct  INTEGER NOT NULL DEFAULT 0 CHECK (total_correct >= 0),
  total_wrong    INTEGER NOT NULL DEFAULT 0 CHECK (total_wrong >= 0),
  last_verdict   TEXT CHECK (last_verdict IS NULL
                   OR last_verdict IN ('correct', 'partial', 'incorrect', 'unreadable')),
  last_seen_at   TEXT,
  PRIMARY KEY (student_id, question_id)
) WITHOUT ROWID;
INSERT INTO mastery_new (
  student_id, question_id, box, due_at, streak, total_correct, total_wrong,
  last_verdict, last_seen_at)
SELECT
  student_id, question_id, box, due_at, streak, total_correct, total_wrong,
  last_verdict, last_seen_at
FROM mastery;
DROP TABLE mastery;
ALTER TABLE mastery_new RENAME TO mastery;
CREATE INDEX idx_mastery_due ON mastery(student_id, due_at);
CREATE INDEX idx_mastery_question ON mastery(question_id);

-- ── concept_weakness ────────────────────────────────────────
CREATE TABLE concept_weakness_new (
  student_id    INTEGER NOT NULL REFERENCES students(id),
  concept_tag   TEXT NOT NULL,
  score         REAL NOT NULL DEFAULT 0 CHECK (score >= 0),
  wrong_count   INTEGER NOT NULL DEFAULT 0 CHECK (wrong_count >= 0),
  last_note     TEXT,
  updated_at    TEXT NOT NULL,
  PRIMARY KEY (student_id, concept_tag)
) WITHOUT ROWID;
INSERT INTO concept_weakness_new (
  student_id, concept_tag, score, wrong_count, last_note, updated_at)
SELECT student_id, concept_tag, score, wrong_count, last_note, updated_at
FROM concept_weakness;
DROP TABLE concept_weakness;
ALTER TABLE concept_weakness_new RENAME TO concept_weakness;
CREATE INDEX idx_cw_score ON concept_weakness(student_id, score DESC);

-- ── ai_calls（除錯日誌，刻意不加外鍵：寫入失敗會被忽略，不能因參照問題遺失紀錄）──
CREATE TABLE ai_calls_new (
  id            TEXT PRIMARY KEY,
  backend       TEXT NOT NULL,
  model         TEXT,
  purpose       TEXT NOT NULL,
  session_id    TEXT,
  question_id   TEXT,
  duration_ms   INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  ok            INTEGER NOT NULL CHECK (ok IN (0, 1)),
  error         TEXT,
  raw_output    TEXT,
  prompt        TEXT,
  input_tokens  INTEGER,
  output_tokens INTEGER,
  created_at    TEXT NOT NULL
);
INSERT INTO ai_calls_new (
  id, backend, model, purpose, session_id, question_id, duration_ms, ok, error,
  raw_output, prompt, input_tokens, output_tokens, created_at)
SELECT
  id, backend, model, purpose, session_id, question_id, duration_ms, ok, error,
  raw_output, prompt, input_tokens, output_tokens, created_at
FROM ai_calls;
DROP TABLE ai_calls;
ALTER TABLE ai_calls_new RENAME TO ai_calls;
CREATE INDEX idx_ai_calls_created ON ai_calls(created_at);
CREATE INDEX idx_ai_calls_session ON ai_calls(session_id);

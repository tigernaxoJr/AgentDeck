CREATE TABLE IF NOT EXISTS students (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  grade       INTEGER NOT NULL DEFAULT 4,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS subjects (
  id          TEXT PRIMARY KEY,          -- 'social' | 'nature' | 'chinese' | 'math'
  name        TEXT NOT NULL,             -- '社會'
  book_title  TEXT NOT NULL DEFAULT '',  -- '四上 社會 評量'
  page_offset INTEGER NOT NULL DEFAULT 0 -- 印刷頁碼 = 掃描檔編號 + page_offset
);

CREATE TABLE IF NOT EXISTS units (
  id          TEXT PRIMARY KEY,          -- '{subject_id}_{序號2位}' 例 'social_01'
  subject_id  TEXT NOT NULL REFERENCES subjects(id),
  title       TEXT NOT NULL,             -- '第 1 回 一、家鄉的地圖與運用 (1)'
  ord         INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS questions (
  id            TEXT PRIMARY KEY,        -- 沿用來源 id，例 'soc_01_tf_1'
  subject_id    TEXT NOT NULL REFERENCES subjects(id),
  unit_id       TEXT NOT NULL REFERENCES units(id),
  ord           INTEGER NOT NULL,        -- 單元內順序
  section       TEXT NOT NULL DEFAULT '',
  qtype         TEXT NOT NULL,           -- 見 §4.1
  stem          TEXT NOT NULL,
  options_json  TEXT NOT NULL DEFAULT '[]',
  answer_json   TEXT NOT NULL,           -- 正規化後答案，見 §4.1
  explanation   TEXT NOT NULL DEFAULT '',
  keywords_json TEXT NOT NULL DEFAULT '[]',
  char_count    INTEGER,                 -- 米字格數
  image_path    TEXT,                    -- 相對 tutor_data/assets，例 'social/crops/social_003_school_map.png'
  book_page     INTEGER,                 -- 實體書頁碼（印刷頁碼）
  book_where    TEXT,                    -- '右上角的校園地圖'
  concept_tags_json TEXT NOT NULL DEFAULT '[]', -- ['方位判斷','地圖-圖例']
  input_mode    TEXT NOT NULL,           -- 'choice' | 'char_grid' | 'free_canvas' | 'image_canvas' | 'text'
  source        TEXT NOT NULL DEFAULT 'import',
  active        INTEGER NOT NULL DEFAULT 1,
  updated_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_q_unit ON questions(unit_id, ord);

CREATE TABLE IF NOT EXISTS sessions (
  id          TEXT PRIMARY KEY,          -- uuid v4
  student_id  INTEGER NOT NULL REFERENCES students(id),
  mode        TEXT NOT NULL,             -- 'progress' | 'review' | 'mixed'
  subject_id  TEXT NOT NULL,
  unit_id     TEXT,
  queue_json  TEXT NOT NULL,             -- ["qid", ...] 本次題目佇列（可在尾端追加重考題）
  cursor      INTEGER NOT NULL DEFAULT 0,
  q_state     TEXT NOT NULL DEFAULT 'asking', -- 目前題目狀態，見 §5
  q_try       INTEGER NOT NULL DEFAULT 0,     -- 目前題目已作答次數
  q_hinted    INTEGER NOT NULL DEFAULT 0,     -- 目前題目是否用過提示 / 書本指引
  started_at  TEXT NOT NULL,
  ended_at    TEXT
);

CREATE TABLE IF NOT EXISTS attempts (
  id               TEXT PRIMARY KEY,     -- uuid
  session_id       TEXT NOT NULL REFERENCES sessions(id),
  student_id       INTEGER NOT NULL,
  question_id      TEXT NOT NULL REFERENCES questions(id),
  try_no           INTEGER NOT NULL,     -- 1,2,3...
  input_kind       TEXT NOT NULL,        -- 'choice' | 'text' | 'handwriting'
  input_text       TEXT,                 -- 選項值或文字
  input_image_path TEXT,                 -- 'answers/<id>.png'
  recognized_text  TEXT,                 -- AI 辨識出的手寫內容
  verdict          TEXT NOT NULL,        -- 'correct' | 'partial' | 'incorrect' | 'unreadable'
  graded_by        TEXT NOT NULL,        -- 'rule' | 'ai'
  created_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_att_student_q ON attempts(student_id, question_id);

CREATE TABLE IF NOT EXISTS dialog_turns (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id   TEXT NOT NULL REFERENCES sessions(id),
  question_id  TEXT NOT NULL,
  role         TEXT NOT NULL,            -- 'tutor' | 'student'
  content      TEXT NOT NULL,
  action       TEXT,                     -- tutor 的 action
  book_ref_json TEXT,
  created_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dlg ON dialog_turns(session_id, question_id, id);

CREATE TABLE IF NOT EXISTS mastery (            -- 每題熟練度（Leitner）
  student_id     INTEGER NOT NULL,
  question_id    TEXT NOT NULL,
  box            INTEGER NOT NULL DEFAULT 0,      -- 0..5
  due_at         TEXT NOT NULL,
  streak         INTEGER NOT NULL DEFAULT 0,      -- 連續一次就對的次數
  total_correct  INTEGER NOT NULL DEFAULT 0,
  total_wrong    INTEGER NOT NULL DEFAULT 0,
  last_verdict   TEXT,
  last_seen_at   TEXT,
  PRIMARY KEY (student_id, question_id)
);
CREATE INDEX IF NOT EXISTS idx_mastery_due ON mastery(student_id, due_at);

CREATE TABLE IF NOT EXISTS concept_weakness (   -- 概念層級弱點
  student_id    INTEGER NOT NULL,
  concept_tag   TEXT NOT NULL,
  score         REAL NOT NULL DEFAULT 0,          -- 越高越弱；答錯 +1，一次答對 ×0.6
  wrong_count   INTEGER NOT NULL DEFAULT 0,
  last_note     TEXT,                             -- AI 寫的誤解描述，例「把太陽與影子方位當成同方向」
  updated_at    TEXT NOT NULL,
  PRIMARY KEY (student_id, concept_tag)
);

CREATE TABLE IF NOT EXISTS ai_calls (            -- 除錯 / 成本追蹤
  id           TEXT PRIMARY KEY,                  -- = runs/<id>
  backend      TEXT NOT NULL,                     -- 'agy' | 'claude' | 'gemini' | 'codex' | <generic id>
  model        TEXT,
  purpose      TEXT NOT NULL,                     -- 'grade_handwriting' | 'tutor_wrong' | 'student_message' | 'enrich_page'
  session_id   TEXT,
  question_id  TEXT,
  duration_ms  INTEGER,
  ok           INTEGER NOT NULL,
  error        TEXT,
  raw_output   TEXT,                              -- turn.json 原文或 stdout 尾端 4KB
  prompt       TEXT,                              -- 送給 AI 的 Prompt
  input_tokens INTEGER, output_tokens INTEGER,
  created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (            -- 執行期可調設定（AI 後端清單等）
  key        TEXT PRIMARY KEY,                    -- 'ai_backends' | 'ai_backends_vision'
  value      TEXT NOT NULL,                       -- 例 'agy,claude'
  updated_at TEXT NOT NULL
);

# AI 家教複習系統（Tutor）設計書

> 目標讀者：負責實作的 agent。每張工單（T-xx）都可獨立交付，依「相依」欄位排序執行。
> 原則：**照抄本文件的介面、欄位名、檔案路徑**；有疑問時選最簡單的實作，不要自行擴充功能。

---

## 0. 需求摘要

| # | 需求 | 設計對應 |
|---|------|----------|
| R1 | 以現有 agy（Antigravity CLI）當 AI 引擎 | 預設後端為 `AgyAdapter`，沿用 `backend-rust` 既有 agy 旗標 |
| R9 | 預留相容各家 AI CLI 的空間 | 獨立 crate `crates/ai-cli`：`AiEngine` trait ＋ `CliAdapter` trait（每家 CLI 一個 adapter），不認識任何業務；見 §6 |
| R10 | 整體使用 VSA + DDD，小模組合併 | 架構規則見 §1.1 |
| R2 | AI 看題目帶學童複習，學童手上有實體書 | 題庫每題記錄 `book_page` / `book_where`，AI 可叫學童「翻到第 N 頁 某處」 |
| R3 | 學童答題，AI 批改並教不會的地方 | 每題一個小型狀態機：作答 → 批改 → 提示 → 再答 → 講解 |
| R4 | 圖片 AI 難以描述 → 叫學童看書 | 有圖的題目，prompt 強制要求以 `book_ref` 指引而非口述圖片 |
| R5 | 國文/數學用手寫板，AI vision 看字、句子、算式 | 前端 Canvas 匯出 PNG → 後端存檔 → 具 vision 能力的 AI 後端讀圖批改 |
| R6 | 記住不熟的地方，之後複習再拿出來問 | SQLite：`mastery`（每題 Leitner 間隔複習）+ `concept_weakness`（概念弱點） |
| R7 | 輕量資料庫 | SQLite 單檔 `tutor.db`（WAL），沿用現有 `sqlx` |
| R8 | 題目參考 `C:\workspace\jamie-exam\data` | 匯入 `jamie-exam/web/public/data/questions.json`（54 題）+ `crops/` 插圖 + `data/scans/` 掃描頁 |

### 既有素材盤點（已確認）

- `C:\workspace\jamie-exam\web\public\data\questions.json`：54 題，社會/自然，欄位
  `id, subject, unit, section, type, stem, options, answer, explanation, keywords, char_count, image, need_handwriting`
  - `type` ∈ `true_false`(21) / `single_choice`(16) / `handwrite_blank`(16) / `handwrite_short_answer`(1)
  - `single_choice.answer` 是 **1-based 整數**；`true_false.answer` 是 `"O"`/`"X"`；`handwrite_*` 是字串
  - `image` 例：`crops/social_003_school_map.png`（檔名中的 `003` 即掃描頁碼）
- `C:\workspace\jamie-exam\web\public\crops\*.png`：15 張插圖切片
- `C:\workspace\jamie-exam\data\scans\社會\`（72 張，含 `社會-解答-095.jpg`…）、`自然\`（50 張）：整頁掃描
- 目前**沒有**國文、數學題庫 → 由 Phase 4 的匯入管線產生，Schema 先設計成通用。

---

## 1. 整體架構

```
┌──────────────────────────── 瀏覽器（電視/平板，大字護眼） ────────────────────────────┐
│ Vue 3  /tutor   （前端同樣按 feature 切片，見 §2）                                       │
│  start（選學童/設定）→ tutor-session（題卡｜書本指引｜作答區｜AI 對話）→ parent-report    │
│  HandwritingPad.vue（米字格 / 自由畫布 / 圖上作答）→ PNG base64                         │
└───────────────┬────────────────────────────────────────────────────────────────────────┘
                │ REST JSON  /api/tutor/*
┌───────────────▼──────────────── backend-rust（binary crate） ──────────────────────────┐
│ tutor/                                                                                  │
│  features/  ← VSA：一個用例一個檔（handler + DTO + 編排）                                │
│   students  catalog  start_session  session_nav  answer  ask_tutor  report  assets      │
│   bank_import  bank_admin  bank_enrich  ai_admin                                        │
│        │ 有業務規則的 slice 才經過 ↓          CRUD slice 直接 sqlx ↘                     │
│  domain/  ← DDD：純資料＋純函式，無 I/O                                    shared/       │
│   question（題目/答案/規則批改）  session（Session 聚合：教學狀態機、選題）  db.rs       │
│   progress（Mastery/ConceptWeakness：間隔複習、弱點）                       dto.rs      │
│  store.rs ← 聚合的載入/儲存（sqlx）                                         error.rs    │
│  ai/      ← 防腐層（ACL）：prompts、AiTurn schema、guard、ai_calls 紀錄、後端設定        │
│        │ 只透過 ai_cli 的公開 API                                                       │
├────────▼────────────────────────────────────────────────────────────────────────────────┤
│ crates/ai-cli（library crate，獨立，不依賴 tutor/sqlx/axum）                            │
│   trait AiEngine ── CliEngine<A: CliAdapter> ── Agy / ClaudeCode / GeminiCli / Codex /   │
│                  │                              Generic(TOML)                            │
│                  ├─ MockEngine（feature "mock"）  └─ (未來) HttpApiEngine                │
│   call_json<T>()：依序容錯＋解析＋重試　　Registry：依設定建立 engines                    │
└───────────────┬───────────────────────────────┬────────────────────────────────────────┘
                │ spawn per call                │
        ┌───────▼──────────────┐        ┌───────▼───────┐
        │ <cli> … "<prompt>"     │ cwd=  │ tutor.db      │
        │ 讀 answer.png          │ runs/ │ (SQLite WAL)  │
        │ 寫 out/turn.json       │ <id>/ └───────────────┘
        └────────────────────────┘
```

### 關鍵設計決策

1. **AI 呼叫是無狀態的**：每次呼叫 AI 都由後端從 DB 組出完整上下文（題目、本題已發生的對話、學童弱點）。不依賴任何 CLI 的對話記憶 → 重啟、換模型、換 CLI 都不影響。
2. **能用規則批改就不叫 AI**：是非/選擇題由後端直接比對答案（毫秒回應）。**答對**用罐頭鼓勵語，**答錯**才叫 AI 教學。手寫/簡答題才需 AI 批改。這把 AI 呼叫次數與等待時間壓到最低。
3. **AI 只輸出結構化 JSON，狀態轉移由 domain 決定**：AI 決定「說什麼」並給「建議動作」；`Session` 聚合依嘗試次數決定實際動作（提示/講解/揭曉），ACL 再做洩答案與頁碼檢查。避免 AI 失控跳題或洩答案。
4. **答案不送到前端**：前端拿到的題目 view 不含 `answer`/`explanation`，直到 domain 決定揭曉。
5. **AI CLI 引擎是獨立 crate**：`crates/ai-cli` 只懂「把 prompt＋檔案交給某個 CLI、拿回 JSON」，不認識學童、題目、資料庫。編譯器保證邊界（它無法 import tutor）。各家 CLI 差異收斂在 `CliAdapter` 的兩個純函式。日後 Console（`executor/runner.rs`）也可改用它（T-22）。
6. **與現有 Console 共存**：同一個 Rust 執行檔、同一個前端，新增 `/api/tutor/*` 與 `/tutor` 路由；Tutor 使用獨立的 `tutor.db`，不動既有 `antigravity_tasks.db`。

### 1.1 架構規則（VSA + 務實 DDD）— 所有工單必須遵守

**VSA（Vertical Slice Architecture）**
1. 以「用例」為單位切檔：`tutor/features/<slice>.rs` 內含該用例的 axum handler、request/response DTO、編排邏輯、**只有它用到的 SQL**。每個 slice 匯出 `pub fn routes() -> Router<TutorState>`，由 `tutor/mod.rs` 合併。
2. **slice 之間不得互相 import**。需要共用的東西只能放在：`domain/`（業務規則）、`store.rs`（聚合存取）、`ai/`（AI 防腐層）、`shared/`（DB 連線、錯誤型別、跨 slice 的 view DTO）。
3. 新增功能 = 新增一個 slice 檔；修改功能 = 只改該 slice（＋必要時 domain）。

**DDD 只用在有業務規則的核心**

| 區塊 | 子域類型 | 做法 |
|---|---|---|
| 教學流程（`Session` 狀態機、選題）、熟練度（間隔複習、弱點）、題目答案判定 | **核心域** | `domain/` 聚合/值物件＋純函式，完整單元測試 |
| 題庫管理（匯入、AI 補頁碼、管理頁編輯） | 支援域 | 一般 slice，直接 sqlx；用到 `domain::question` 的正規化函式即可 |
| 學童、科目目錄、報表、設定 | CRUD | 一般 slice，直接 sqlx，**不建 entity/repository/service** |
| AI CLI 呼叫 | 通用子域 | 獨立 crate `ai-cli`；tutor 端以 `tutor/ai/` 當防腐層，把 AI 的 JSON 轉成 domain 概念 |

**DDD 戰術元素的取捨**
- 用：聚合（`Session`、`StudentProgress`）、值物件（`Answer`、`Verdict`、`BookRef`）、聚合方法回傳「結果 struct」由 slice 寫回 DB。
- 不用：domain event bus、CQRS、Unit of Work、repository trait、application service 層、DI 容器。一次請求的寫入由 slice 在**單一 sqlx transaction** 完成。
- `domain/` 為純程式：**不得** import `sqlx`、`axum`、`tokio`、`ai_cli`、`std::fs`；目前時間以參數 `now: DateTime<Utc>` 傳入；亂數以 `seed` 傳入。

**大小與合併規則**
- 預期 **< ~80 行**、或**只有一個呼叫者**的模組 → 併入呼叫者，不另開檔。檔案超過 **~400 行**才考慮拆。
- 只有一個實作的東西**不建 trait**。例外：跨程序/外部系統邊界（`AiEngine`、`CliAdapter`）。
- 測試替身優先用 SQLite `:memory:` 與 `ai_cli::MockEngine`，不要為了 mock 而抽 trait。
- 同性質的極小 slice 合併（例：取得 session / 下一題 / 結束 → `session_nav.rs`）。

---

## 2. 目錄結構（新增部分）

```
backend-rust/
  Cargo.toml                       # 新增 ai-cli = { path = "crates/ai-cli" }
  crates/ai-cli/                   # ── 通用子域：AI CLI 引擎（library crate）──
    Cargo.toml                     # 依賴：tokio, serde, serde_json, async-trait, toml；features = ["mock"]
    src/
      lib.rs                       # [T-07] 公開 API re-export
      engine.rs                    # [T-07] AiEngine trait、Capabilities、VisionMode、AiInvocation、AiRawResult、AiError、TokenUsage
      cli.rs                       # [T-07] CliAdapter trait、CommandSpec、StreamItem、CliEngine<A>、resolve/which
      call.rs                      # [T-07] call_json<T>()：依序容錯、解析、重試；回 CallRecord 清單（不碰 DB）
      registry.rs                  # [T-07] Registry：依 BackendConfig 建 Vec<Arc<dyn AiEngine>>、health check
      mock.rs                      # [T-07] MockEngine（cfg(any(test, feature="mock"))）
      adapters/agy.rs              # [T-07] AgyAdapter（預設；含 resolve_agy_bin，搬自 runner.rs）
      adapters/claude_code.rs      # [T-08]
      adapters/gemini_cli.rs       # [T-08]
      adapters/codex.rs            # [T-08]
      adapters/generic.rs          # [T-08] 由 ai_backends.toml 設定的任意 CLI
      adapters/README.md           # [T-07] 新增一家 CLI 的步驟
    tests/fake_cli/                # [T-07] 假 CLI
    tests/fixtures/<id>_stream.txt # [T-00/T-08] 各 CLI 真實輸出樣本
  src/
    executor/runner.rs             # [T-07] 改呼叫 ai_cli::adapters::agy::resolve_agy_bin()（其餘不動）
    tutor/
      mod.rs                       # [T-10] TutorState、routes() 合併所有 slice、啟動初始化
      shared/
        db.rs                      # [T-01] open()、MIGRATIONS（PRAGMA user_version 版本化遷移）
        migrations/                # [T-01] 0001_init.sql 基準、0002_constraints.sql 約束/索引/question_concepts
        dto.rs                     # [T-05] 跨 slice 的 view：QuestionView、SessionView、TutorReply、BookRef
        error.rs                   # [T-05] ApiError（→ axum IntoResponse，{error, message}）
      domain/                      # ── 核心域：純程式 ──
        question.rs                # [T-02] Question、QType、InputMode、Answer、grade_choice、answer_display、contains_answer、normalize_import
        progress.rs                # [T-03] Mastery、ConceptWeakness、Outcome、next_mastery、update_weakness
        session.rs                 # [T-04] Session 聚合（狀態機）、TeachAction、Step、select_queue
      store.rs                     # [T-05] 聚合載入/儲存：load_session/save_session、load_progress/save_progress、load_question
      ai/                          # ── 防腐層 ──
        prompts.rs                 # [T-09] prompt 模板（依 Capabilities）
        turn.rs                    # [T-09] AiTurn schema、call_turn()（包 ai_cli::call_json）、guard、寫 ai_calls、罐頭回覆
        backends.rs                # [T-09] settings 表 ↔ ai_cli::Registry、engines_for(purpose)
      features/                    # ── VSA slices ──
        students.rs                # [T-10] GET/POST /students（CRUD）
        catalog.rs                 # [T-10] GET /subjects（CRUD 查詢）
        assets.rs                  # [T-10] GET /assets/*（防 path traversal）
        bank_import.rs             # [T-06] POST /admin/import
        start_session.rs           # [T-11] POST /sessions（查候選 → domain::session::select_queue）
        session_nav.rs             # [T-11] GET /sessions/:id、POST next、POST end（合併的小 slice）
        answer.rs                  # [T-11] POST answer、POST self_grade
        ask_tutor.rs               # [T-11] POST message
        report.rs                  # [T-12] GET /students/:id/report（CRUD 查詢）
        bank_admin.rs              # [T-12] GET/PATCH /admin/questions/:id
        ai_admin.rs                # [T-12] /admin/ai/backends、/admin/ai/test
        bank_enrich.rs             # [T-17] POST /admin/enrich
tutor_data/                        # 執行期資料（加入 .gitignore）
  tutor.db
  ai_backends.toml                 # 可選，GenericAdapter 設定
  assets/<subject_id>/crops/*.png  # 匯入時複製
  assets/<subject_id>/scans/*.jpg  # 匯入時複製（給 AI 看整頁用）
  answers/<attempt_id>.png         # 學童手寫原圖（保留，給家長看）
  runs/<call_id>/                  # AI CLI 每次呼叫的工作目錄（7 天後清除）
frontend-vue/src/
  router.ts                        # [T-13] vue-router：/ (既有 Console，不搬動)、/tutor/*
  tutor/
    shared/  http.ts  types.ts  BigButton.vue  theme.css      # [T-13] 跨 feature 共用
    features/
      start/          StudentPicker.vue  SessionSetup.vue  api.ts            # [T-16]
      tutor-session/  TutorSession.vue  useTutorSession.ts  api.ts
                      QuestionCard.vue  BookRefCallout.vue  AnswerPanel.vue
                      TutorBubble.vue  HandwritingPad.vue                    # [T-14][T-15]
      parent-report/  ParentReport.vue  api.ts                               # [T-16]
      admin/          QuestionAdmin.vue  AiBackends.vue  api.ts              # [T-16]
```

前端同樣遵守 §1.1：feature 之間不互相 import，共用只能放 `tutor/shared/`；只被一個 feature 用的元件（例如 `HandwritingPad`）放在該 feature 內。

---

## 3. 資料庫設計（`backend-rust/src/tutor/shared/migrations/`，此節為 v1 基準；v2 起的約束見 `0002_constraints.sql`）

啟動時執行 `PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;`，DDL 全部 `CREATE TABLE IF NOT EXISTS`。時間一律存 **UTC ISO-8601 字串**（`chrono::Utc::now().to_rfc3339()`）。

```sql
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
  input_tokens INTEGER, output_tokens INTEGER,
  created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (            -- 執行期可調設定（AI 後端清單等）
  key        TEXT PRIMARY KEY,                    -- 'ai_backends' | 'ai_backends_vision'
  value      TEXT NOT NULL,                       -- 例 'agy,claude'
  updated_at TEXT NOT NULL
);
```

---

## 4. 題庫

### 4.1 題型與正規化（匯入時轉換）

| 來源 `type` | `qtype` | `input_mode` | `answer_json` 正規化 | 批改方式 |
|---|---|---|---|---|
| `true_false` | `true_false` | `choice` | `"O"` 或 `"X"` | 規則 |
| `single_choice` | `single_choice` | `choice` | **0-based 索引**：來源 `2` → `1` | 規則 |
| `handwrite_blank` | `fill_blank` | `char_grid`（有 `image` 時 `image_canvas`） | 字串，例 `"東北"` | AI vision；**另提供「改用選項作答」按鈕**（若 `options` 非空）→ 規則 |
| `handwrite_short_answer` | `short_answer` | `free_canvas` | 字串（參考答案） | AI vision，需含 `keywords` |
| （新）`math_expr` | `math_expr` | `free_canvas` | `{"final":"36","steps_hint":"..."}` | AI vision：逐行檢查算式 |
| （新）`chinese_write` | `chinese_write` | `char_grid` | 字串（要寫的字/詞/句） | AI vision：錯字、漏字、字形明顯錯誤 |
| （新）`text` 類 | `short_answer` | `text` | 字串 | AI（無圖） |

> 國文：AI vision 能判斷「錯字、別字、漏字、明顯多/少筆劃」，**無法判斷筆順**，文件與 UI 都不要宣稱能。

### 4.2 匯入規則（`features/bank_import.rs`，T-06；欄位正規化函式在 `domain/question.rs`）

- 來源根目錄由環境變數 `TUTOR_IMPORT_ROOT`（預設 `C:\workspace\jamie-exam`）。
- `subject`：`社會`→`social`、`自然`→`nature`、`國文`→`chinese`、`數學`→`math`。
- `unit`：依出現順序編號 `{subject_id}_{01..}`；`ord` 為出現順序。
- `image`：`crops/xxx.png` → 複製到 `tutor_data/assets/{subject_id}/crops/xxx.png`，`image_path` 存相對路徑。
- `book_page` 推導：`image` 檔名符合 `_(\d{3})_` → 該數字 + `page_offset`；否則同一 `unit` 內**前一題**有頁碼就沿用；都沒有則 `NULL`（由 T-17 補，或家長在管理頁填）。
- `book_where`：匯入時留空。
- `concept_tags`：匯入時以 `section` 與 `keywords` 粗略填入；T-17 會用 AI 補強。
- 掃描頁：`data/scans/{社會|自然}/*.jpg` 複製到 `tutor_data/assets/{subject_id}/scans/`。
- **冪等**：以 `id` UPSERT；若 `book_page`/`book_where`/`concept_tags_json` 已有人工值（非 NULL/非空），不要覆寫。

---

## 5. 教學狀態機（`domain/session.rs`，T-04）

每個 session 同時只有一題「目前題目」，狀態存在 `sessions.q_state / q_try / q_hinted`。`Session` 聚合以純方法表達下列轉移：`fn apply_verdict(&mut self, v: Verdict, now) -> Step`，`Step { state, action: TeachAction(Praise|Hint|Explain|RetryUnreadable), reveal: bool, outcome: Option<Outcome>, requeued: bool }`；slice 依 `Step` 決定是否呼叫 AI 與如何回應。「每題最多追加 1 次」由 queue 中該 id 出現次數判斷，不另存欄位。

```
             ┌──────────── answer(correct) ─────────────► REVEALED ──next──► 下一題 ASKING
             │
ASKING ──answer──► 批改
             │
             ├─ incorrect & q_try==1 ──► HINTING（AI：提示，不洩答案；有圖→book_ref）──► ASKING
             ├─ incorrect & q_try>=2 ──► EXPLAINED（AI：講解＋揭曉答案＋book_ref）
             │                               └─ 本題追加到 queue 尾端（同 session 再考一次，每題最多追加 1 次）
             │                               └─ ──next──► 下一題
             ├─ partial（簡答部分正確）──► 視同 incorrect 但 AI 語氣為「補完整」
             └─ unreadable（手寫看不懂）──► ASKING（不計 q_try，請學童寫大一點）

任何狀態：student_message（學童打字/說「我不懂」「為什麼」）──► AI 回答 ──► 狀態不變
```

**覆寫規則（AI 的 `action` 只是建議；action 由 `Session` 決定、洩答檢查在 `tutor/ai/turn.rs`）：**
- `q_try==1` 且答錯 → 強制 `hint`，且送前端前**移除** `say` 中出現的正確答案字串（簡單 `contains` 檢查；命中則改用罐頭提示「再想想看，可以翻翻課本喔！」並記 log）。
- `q_try>=2` 答錯 → 強制 `explain`，回傳 `reveal: {answer, explanation}`。
- 答對 → `reveal` 一併回傳（讓學童看到解析）。

### 5.1 熟練度更新（`domain/progress.rs`，T-03）

每題「結束」（進入 REVEALED 或 EXPLAINED）時更新一次 `mastery`：

| 本題結果 | box | streak | due_at |
|---|---|---|---|
| 第一次就對 | `min(box+1, 5)` | +1 | now + INTERVAL[box] |
| 提示後才對（q_try≥2 對） | `max(box-1, 0)` 但至少 1 | 0 | now + 1 天 |
| 最終仍錯（EXPLAINED） | 0 | 0 | now + 10 分鐘（同日還會再遇到） |

`INTERVAL = [10分鐘, 1天, 2天, 4天, 7天, 14天]`（索引為新 box）。

`concept_weakness`（對該題每個 concept_tag，以及 AI 回傳的 `concept_tags`）：
- 最終仍錯 / 提示後才對：`score += 1`（提示後才對 `+0.5`），`wrong_count += 1`，`last_note = AI 的 misconception`。
- 第一次就對：`score *= 0.6`；`score < 0.2` 時設為 0。

### 5.2 選題（`domain/session.rs::select_queue`，T-04；候選題目查詢在 `features/start_session.rs`）

最多 `N = 15` 題（可由請求參數覆寫）。

- `progress`（循序新進度）：指定 `unit_id`，該單元 `active=1` 的題目依 `ord`，**略過** `mastery.box >= 3` 的題目。
- `review`（弱點複習）：
  1. `mastery.due_at <= now` 的題目，依 `box ASC, due_at ASC`；
  2. 不足 N → 補「含有 `concept_weakness.score >= 1` 標籤」且未在清單中的題目（依 score DESC）；
  3. 仍不足 → 補 `total_wrong > 0` 的題目。
  4. 結果為空 → 回 `{"empty": true, "message": "目前沒有需要複習的題目 🎉"}`。
- `mixed`：`review` 取 `ceil(N*0.6)`，其餘用 `progress` 規則（從第一個尚有 box<3 題目的單元開始）。
- 最後用 session id 當種子打散（`review` 才打散；`progress` 保持順序）。

---

## 6. AI 引擎（`crates/ai-cli`，T-07 / T-08）與防腐層（`tutor/ai/`，T-09）

### 6.0 分層與 trait 設計

```
tutor/features/*（answer、ask_tutor、bank_enrich）
      │  只呼叫 tutor::ai::turn::call_turn / ai_cli::call_json
      ▼
┌──────────────────────── trait AiEngine ────────────────────────┐  ← 業務層唯一依賴
│ CliEngine<A: CliAdapter>   MockEngine   (未來) HttpApiEngine     │
└──────────┬─────────────────────────────────────────────────────┘
           │ CliEngine 負責「所有 CLI 共通」的事：建 run 目錄、spawn、timeout、
           │ 收 stdout/stderr、讀 out/*.json、kill_on_drop
           ▼
┌──────────────────────── trait CliAdapter ──────────────────────┐  ← 每家 CLI 的差異只在這
│ AgyAdapter  ClaudeCodeAdapter  GeminiCliAdapter  CodexAdapter    │
│ GenericAdapter（由 TOML 設定任意 CLI）                            │
│ 核心只有兩個純函式：build_command()、parse_line() → 不碰 I/O，好寫好測 │
└────────────────────────────────────────────────────────────────┘
```

**兩層 trait 的理由**：`AiEngine` 讓「不是 CLI 的東西」（直接打 HTTP API、測試替身）也能接上；`CliAdapter` 讓新增一家 CLI 只要寫「怎麼組指令、怎麼讀輸出」，不必重寫行程管理。多後端容錯由 `call_turn` 依序嘗試 `Vec<Arc<dyn AiEngine>>` 完成（見下方），因為每換一個後端都要依它的能力**重新組 prompt**。

#### `crates/ai-cli/src/engine.rs`

```rust
/// 這家後端能做什麼。tutor/ai/prompts.rs 與教學 slices 依此調整行為。
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub vision: VisionMode,          // 能否看圖、圖怎麼給
    pub reads_files: bool,           // 能否自行讀 cwd 內的檔案（context.json）
    pub writes_files: bool,          // 能否把結果寫成 ./out/turn.json
    pub max_prompt_chars: usize,     // 超過時 prompts.rs 要裁短
}

#[derive(Debug, Clone, Serialize)]
pub enum VisionMode {
    None,                            // 不能看圖 → 手寫題自動改走「自評」
    PromptPath,                      // 在 prompt 寫 "./answer.png"，CLI 自己用讀檔工具開
    AtMention,                       // 在 prompt 寫 "@answer.png"
    Flag { flag: String },           // 每張圖加一組 `<flag> <path>`，例 codex 的 "-i"
}

pub struct AiInvocation {
    pub call_id: String,             // = runs/<call_id>
    pub purpose: AiPurpose,          // GradeHandwriting | TutorWrong | StudentMessage | EnrichPage
    pub prompt: String,              // 已依 capabilities 組好（圖片引用方式已套用）
    pub files: Vec<RunFile>,         // 放進 run 目錄的檔案
    pub images: Vec<String>,         // files 中哪些是圖片（相對檔名），給 Flag 模式用
    pub expect_output_file: Option<String>, // 例 Some("out/turn.json")
    pub model: Option<String>,
    pub timeout: Duration,
}
pub struct RunFile { pub name: String, pub bytes: Vec<u8> }

pub struct AiRawResult {
    pub backend_id: String,
    pub output_file: Option<String>, // expect_output_file 的內容（若有）
    pub final_text: Option<String>,  // CLI 最終回覆文字
    pub usage: Option<TokenUsage>,   // crate 自己定義（欄位同 models::protocol::TokenUsage），不依賴 binary crate
    pub duration: Duration,
    pub stderr_tail: String,         // 最後 4KB，除錯用
}

#[derive(Debug)]                     // 手寫 Display，不新增 thiserror 依賴
pub enum AiError {
    NotInstalled(String), Spawn(String), Timeout,
    NonZeroExit { code: Option<i32>, stderr_tail: String },
    EmptyOutput, Unsupported(String),
}

#[async_trait::async_trait]          // 新增依賴 async-trait = "0.1"（dyn trait 需要）
pub trait AiEngine: Send + Sync {
    fn id(&self) -> &str;                          // "agy" | "claude" | ...
    fn capabilities(&self) -> &Capabilities;
    fn model(&self) -> Option<String>;             // 此後端設定的 model
    async fn health_check(&self) -> Result<String, AiError>; // 跑 `--version`，回版本字串
    async fn run(&self, inv: AiInvocation) -> Result<AiRawResult, AiError>;
}
```

#### `crates/ai-cli/src/cli.rs`

```rust
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<String>,       // 有些 CLI 可從 stdin 吃 prompt（避開 Windows 命令列長度上限）
}

pub enum StreamItem { TextDelta(String), FinalText(String), Usage(TokenUsage), Ignore }

pub trait CliAdapter: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn default_program(&self) -> String;            // agy 用 adapters::agy::resolve_agy_bin()
    fn capabilities(&self) -> Capabilities;
    fn version_args(&self) -> Vec<String> { vec!["--version".into()] }
    /// 純函式：依 invocation 組出指令（cwd 由 CliEngine 設為 run 目錄）
    fn build_command(&self, program: &str, inv: &AiInvocation, run_dir: &Path) -> CommandSpec;
    /// 純函式：解析 stdout 的一行
    fn parse_line(&self, line: &str) -> StreamItem;
}
```

`CliEngine<A>::run()` 固定流程：
1. 建 `runs/<call_id>/` 與 `runs/<call_id>/out/`，寫入 `files`。
2. `adapter.build_command(...)` → `tokio::process::Command`，`current_dir(run_dir)`、`kill_on_drop(true)`；stdin 依 `CommandSpec.stdin`（None 則 `Stdio::null()`）。
3. 逐行讀 stdout 丟給 `parse_line`：累積 `TextDelta`；`FinalText` 覆蓋；`Usage` 記下。stderr 只留尾端 4KB。
4. `timeout` 到 → kill → `AiError::Timeout`；program 找不到 → `NotInstalled`；非 0 結束且無任何輸出 → `NonZeroExit`。
5. 讀 `expect_output_file`（存在才讀）→ 組 `AiRawResult`。`final_text` = FinalText ?? 累積的 TextDelta。

#### `ai_cli::call_json`（crate 內，通用）與 `tutor/ai/turn.rs::call_turn`（ACL）

```rust
// ── crates/ai-cli/src/call.rs（通用，不認識 tutor、不碰 DB）──
pub struct CallRecord { pub backend: String, pub model: Option<String>, pub duration: Duration,
                        pub usage: Option<TokenUsage>, pub ok: bool, pub error: Option<String>, pub raw_tail: String }
pub struct CallSpec<'a> {
    pub needs_vision: bool,
    pub build_prompt: &'a (dyn Fn(&Capabilities) -> String + Send + Sync), // 每換一個後端就依其能力重組 prompt
    pub files: Vec<RunFile>, pub images: Vec<String>,
    pub output_file: &'a str,               // 例 "out/turn.json"
    pub timeout: Duration,
}
pub async fn call_json<T: DeserializeOwned>(engines: &[Arc<dyn AiEngine>], spec: CallSpec<'_>)
    -> (Result<T, AiError>, Vec<CallRecord>);

// ── backend-rust/src/tutor/ai/turn.rs（防腐層）──
pub async fn call_turn(backends: &Backends, purpose: AiPurpose, ctx: &PromptContext,
                       files: Vec<RunFile>, conn: &SqlitePool) -> Result<TutorFeedback, TurnError>;
```

`call_json` 對清單中每個 engine：
1. `spec.needs_vision` 而 `capabilities().vision == None` → 跳過此 engine。
2. `(spec.build_prompt)(engine.capabilities())` → `engine.run()`。
3. 解析：① `output_file` ② `final_text` 中第一個 `{` 到最後一個 `}` → serde 成 `T`。解析失敗在**同一 engine** 重試 1 次（prompt 尾加「上次輸出不是合法 JSON，請只輸出 JSON」）。
4. `NotInstalled / Spawn / Timeout / NonZeroExit / EmptyOutput` 或兩次解析失敗 → 換下一個 engine。
5. 每次 `run` 產生一筆 `CallRecord`。

**分工**：`ai_cli::call_json<T>` 只負責「依序容錯＋解析＋重試」，不論成敗都回傳 `Vec<CallRecord>`。`tutor/ai/turn.rs::call_turn` 是防腐層：依 purpose 從 `backends.engines_for()` 取清單、以 `prompts::build_prompt` 當 `build_prompt` 閉包、呼叫 `call_json::<AiTurn>`、把 `CallRecord` 寫入 `ai_calls`、做洩答/頁碼 guard，最後轉成 domain 用的 `TutorFeedback { verdict, say, book_ref, recognized_text, misconception, concept_tags, followup }`。

全部失敗回 `Err`，slice 改用罐頭回覆（§6.4）；若是因為沒有任何具 vision 的後端，回 `selfGradeAvailable=true`。

#### 能力驅動的行為（tutor/ai/prompts.rs 與教學 slices 必須遵守）

| Capability | 為 true / 有值 | 為 false / None |
|---|---|---|
| `reads_files` | prompt 寫「請先讀 ./context.json」 | 把 context.json 內容**直接貼進 prompt**（json 區塊） |
| `writes_files` | 要求寫 `./out/turn.json` | 要求「只輸出一個 JSON 物件，不要其他文字」，從 `final_text` 擷取 |
| `vision` | `PromptPath`→`./answer.png`、`AtMention`→`@answer.png`、`Flag`→prompt 寫「附件圖片 answer.png」並由 adapter 加旗標 | `None`：手寫題不送此後端；題目插圖不提供 |
| `max_prompt_chars` | — | 超過時依序裁掉：dialog 舊訊息 → weak_concepts → explanation |

`prompts.rs` 提供 `fn image_ref(caps: &Capabilities, file: &str) -> String`，模板中所有圖片檔名都經由它輸出，**模板內不得寫死 `./answer.png`**；同理 `fn output_instruction(caps)`、`fn context_instruction(caps, ctx_json)`。

### 6.1 各家 Adapter 規格

> ⚠ 除 agy（已在本 repo `runner.rs` 使用）外，其餘旗標依各 CLI 公開文件撰寫，**實作 T-08 時必須先跑 `<cli> --help` 核對**；不符就以實測為準，並更新本表。

| id | 預設 program | 指令（`{p}`=prompt，`{m}`=model） | vision | reads / writes | 輸出解析 |
|---|---|---|---|---|---|
| `agy`（預設） | `resolve_agy_bin()`（`agy`→`antigravity`） | `--dangerously-skip-permissions --output-format stream-json [--model {m}] -p {p}` | 依 T-00 結果（預期 `PromptPath`） | ✅ / ✅ | NDJSON：`/step_update/text_delta`→TextDelta、`/result/response`→FinalText、`/result/usage`｜`/step_update/usage`｜`/usage`→Usage（同 runner.rs） |
| `claude` | `claude` | `-p {p} --output-format stream-json --verbose --dangerously-skip-permissions [--model {m}]` | `PromptPath` | ✅ / ✅ | NDJSON：`type=="result"` 的 `/result`→FinalText、`/usage`→Usage |
| `gemini` | `gemini` | `-p {p} --yolo [-m {m}]`（若版本支援 `--output-format json` 則加上） | `AtMention` | ✅ / ✅ | 純文字：每行 TextDelta；JSON 模式取 `/response` |
| `codex` | `codex` | `exec --full-auto --skip-git-repo-check [-m {m}] [-i <img>]… {p}` | `Flag{"-i"}` | ✅ / ✅ | 純文字 → TextDelta |
| `generic` | 設定檔 | 設定檔模板 | 設定檔 | 設定檔 | `line_mode = text \| ndjson` + JSON pointer |

Windows 注意：命令列長度上限約 32K 字元。prompt **超過 8000 字元**時，adapter 若支援 stdin 就改走 stdin；否則 `reads_files` 必須為 true（context 放檔案，prompt 保持短）。

#### `GenericAdapter` 設定檔：`tutor_data/ai_backends.toml`

不寫 Rust 也能接新 CLI（新增依賴 `toml = "0.8"`）：

```toml
[[backend]]
id = "my-cli"
program = "mycli"
args = ["run", "--yes", "{model_args}", "{image_args}", "--prompt", "{prompt}"]
model_args = ["--model", "{model}"]     # model 為空時整組移除
image_args = ["--attach", "{image}"]    # 每張圖展開一次；vision 不是 flag 時忽略
vision = "flag"                         # none | prompt_path | at_mention | flag
reads_files = true
writes_files = true
max_prompt_chars = 30000
prompt_via_stdin = false                # true 時 {prompt} 不放 args，改寫入 stdin
line_mode = "ndjson"                    # text | ndjson
final_text_pointer = "/result"          # 僅 ndjson
text_delta_pointer = "/delta"
usage_pointer = "/usage"
version_args = ["--version"]
```

`args` 中的 `{model_args}` / `{image_args}` 是**整組展開**的佔位元素；其他字串內的 `{prompt}`、`{model}`、`{image}` 做文字替換。不經過 shell（直接 `Command::new`），不支援管線或重導向。

#### `crates/ai-cli/src/registry.rs` ＋ `tutor/ai/backends.rs`：選擇後端

- crate 的 `Registry::build(cfg: &BackendConfig)` 只吃設定 struct（id 清單、各後端 model / bin 覆寫、toml 路徑），**不讀 DB、不讀環境變數**；讀取設定是 `tutor/ai/backends.rs` 的事。
- 設定來源優先序（`backends.rs`）：DB `settings` 表（管理頁可即時切換）> 環境變數 > 預設 `"agy"`。
- `ai_backends`（環境變數 `TUTOR_AI_BACKENDS`）：逗號分隔清單，例 `agy,claude` → 依序容錯。
- `ai_backends_vision`（`TUTOR_AI_BACKENDS_VISION`）：看圖用途（手寫批改、enrich）專用清單；未設定則同主清單。例：平常用便宜的 A，看圖用 B。
- 內建 id：`agy`、`claude`、`gemini`、`codex`；其他 id 到 `ai_backends.toml` 找；找不到 → log 警告並略過。
- 每個後端的 model：`TUTOR_MODEL_<ID 大寫，`-`→`_`>`（例 `TUTOR_MODEL_AGY`），未設定用 CLI 預設。
- 啟動時對每個後端跑 `health_check()`，結果存記憶體；`NotInstalled` 的後端從清單移除。清單變成空的 → 啟動不失敗，所有 AI 功能走罐頭/自評，並在管理頁顯示警告。
- 對外提供：`fn engines_for(&self, purpose: AiPurpose) -> Vec<Arc<dyn AiEngine>>`；以 `ArcSwap` 或 `RwLock` 持有，`PUT /admin/ai/backends` 時重建。

#### 新增一家 CLI 的步驟（寫進 `crates/ai-cli/src/adapters/README.md`）

1. 複製 `adapters/agy.rs` 為 `adapters/<id>.rs`，實作 `CliAdapter`。
2. 在 `registry.rs` 的 `match id { … }` 加一行。
3. 單元測試：`build_command` 的 args 快照；`parse_line` 餵真實輸出樣本 `tests/fixtures/<id>_stream.txt`。
4. 冒煙：`cargo test -- --ignored real_cli_<id>`。
5. **不得**修改 `crates/ai-cli` 以外的任何檔案；若非改不可，代表抽象漏了 → 回報，不要硬改。

### 6.1b 呼叫共通規則

```
cwd     = tutor_data/runs/<call_id>/          （每次新建，只放這次需要的檔案）
files   = context.json                         （題目＋對話＋弱點）
          question.png   （題目有 image_path 且 vision≠None 時）
          answer.png     （學童手寫）
          page.jpg       （僅 enrich）
輸出    = writes_files ? ./out/turn.json : 最終回覆文字中的 JSON
timeout = TUTOR_AI_TIMEOUT_SECS（預設 90）
```

- 同一 session 同時只允許一個 AI 呼叫（`DashMap<session_id, ()>` 當鎖）；重複請求回 `409`。全域 `Semaphore(TUTOR_AI_CONCURRENCY=2)`，跨所有後端共用。
- `runs/` 下超過 7 天的目錄於啟動時刪除。
- 安全：各家「跳過權限確認」旗標（`--dangerously-skip-permissions`、`--yolo`、`--full-auto`）只在 `runs/<id>/` 內執行；run 目錄不放 DB、其他學童資料或任何 secret。

### 6.2 AI 輸出 JSON（`AiTurn`）

```json
{
  "verdict": "correct | partial | incorrect | unreadable | none",
  "recognized_text": "學童手寫辨識結果（沒有手寫就給空字串）",
  "say": "要對學童說的話，國小四年級用語，2~4 句，最多 120 字",
  "action": "praise | hint | explain | show_book | ask_followup | answer_question",
  "book_ref": { "page": 7, "where": "右上角的表格" },
  "misconception": "一句話描述學童可能的錯誤觀念；沒有就空字串",
  "concept_tags": ["方位判斷"],
  "followup_question": "可選：一個確認理解的小問題；沒有就空字串"
}
```

- `book_ref` 可為 `null`；`page` 必須等於 context 中題目的 `book_page`（或 enrich 提供的頁），engine 若發現 AI 給的頁碼不在允許清單 → 改用題目的 `book_page`。
- `verdict="none"` 用於 `student_message`（純問答）。

### 6.3 Prompt 模板（`tutor/ai/prompts.rs`，純函式 + 單元測試）

共同 System 區塊（每次呼叫都放在最前面）：

```
你是一位溫柔、有耐心的國小家教老師，正在陪一位{grade}年級的學童複習「{subject_name}」。
學童手上有實體課本/評量《{book_title}》。

規則：
1. 用繁體中文、國小{grade}年級聽得懂的話，句子短，2~4 句。多鼓勵，不要說「你錯了」，改說「差一點點」。
2. 題目有圖（{question_image}）時，你可以看圖理解，但「不要用文字描述圖片細節」；
   需要學童看圖時，用 book_ref 叫學童翻到課本第幾頁的哪個位置，say 裡也要說出頁碼與位置。
3. 給提示（hint）時絕對不能說出正確答案或答案中的字。
4. {context_instruction}
5. {output_instruction}
{AI_TURN_SCHEMA}
```

佔位字由 `prompts.rs` 依 `Capabilities` 產生（§6.0 能力驅動表）：
- `{question_image}` = `image_ref(caps, "question.png")`；題目無圖或 `vision=None` 時，整條規則 2 改為「這題的圖請一律用 book_ref 叫學童看課本」。
- `{context_instruction}`：`reads_files` → 「檔案 ./context.json 有題目、正確答案、解析、這題之前的對話、學童過去的弱點，請先讀它。」；否則 → 「以下是題目、正確答案、解析、之前的對話與學童弱點：」+ json 區塊。
- `{output_instruction}`：`writes_files` → 「只把結果以 JSON 寫入 ./out/turn.json（UTF-8），格式如下，不要輸出其他檔案：」；否則 → 「只輸出一個 JSON 物件，不要有任何其他文字或 markdown，格式如下：」。

各用途的任務區塊（接在 System 後）：

- **`grade_handwriting`**（手寫題批改＋教學，一次呼叫完成）：
  ```
  任務：{answer_image} 是學童用手寫板寫的答案。（{answer_image} = image_ref(caps, "answer.png")）
  1. 先辨識手寫內容填入 recognized_text；字跡無法辨識時 verdict=unreadable，請學童寫大一點、慢一點。
  2. 與正確答案比對給 verdict。{qtype_specific}
  3. 這是第 {try_no} 次作答。答對 → action=praise，簡短說明為什麼對。
     第 1 次答錯 → action=hint 或 show_book，只給提示。第 2 次以上答錯 → action=explain，清楚講解並說出答案。
  ```
  `qtype_specific`：
  - `fill_blank`：「同音字、錯別字都算錯，但要在 say 中指出是哪個字寫錯。」
  - `chinese_write`：「檢查錯字、別字、漏字、多字、明顯多或少筆劃；不評筆順。指出錯在哪個字、哪個部件。」
  - `short_answer`：「意思對、包含關鍵概念 {keywords} 即可算 correct；只對一部分算 partial。」
  - `math_expr`：「逐行辨識算式，找出第一個出錯的步驟（抄錯、進位、計算、單位），最終答案 {final}。過程錯但答案對算 partial。」
- **`tutor_wrong`**（規則批改判定答錯後的教學；verdict 已知）：
  ```
  任務：學童選了「{student_choice}」，這是錯的（第 {try_no} 次）。verdict 請填 incorrect。
  依規則 3 給提示或講解。請推測學童的錯誤觀念寫在 misconception。
  ```
- **`student_message`**：
  ```
  任務：學童對你說：「{message}」。目前題目狀態：{q_state}（若為 asking / hinting，不能洩漏答案）。
  回答學童，verdict=none，action=answer_question 或 show_book。
  ```
- **`enrich_page`**（T-17 離線用）：prompt 寫在 `features/bank_enrich.rs`，見 T-17。

`context.json` 內容：
```json
{
  "student": {"name": "小明", "grade": 4},
  "question": {"id":"...","qtype":"...","stem":"...","options":[...],"answer":...,"explanation":"...",
               "keywords":[...],"book_page":3,"book_where":"右上角的校園地圖","has_image":true},
  "try_no": 2,
  "dialog": [{"role":"tutor","content":"..."},{"role":"student","content":"..."}],   // 本題最近 10 則
  "weak_concepts": [{"tag":"方位判斷","score":2.5,"note":"..."}]                       // score 最高 5 個
}
```

### 6.4 罐頭回覆（AI 失敗或不需要 AI 時）

- 答對（規則批改）：隨機挑 `["答對了！好棒 🎉","沒錯！你真厲害！","完全正確，繼續加油！"]`，`action=praise`。
- AI 失敗 + 第 1 次錯：`"差一點點！{有 book_page ? "翻到課本第 {p} 頁{where}看看，" : ""}再想想看。"`
- AI 失敗 + 第 2 次錯：`"我們一起看答案：{answer}。{explanation}"`
- 手寫題 AI 失敗：讓前端顯示「自己對答案」按鈕（顯示標準答案疊圖，學童/家長按 ✅/❌ 自評 → `graded_by='self'`）。

---

## 7. HTTP API（`/api/tutor`，T-10 ~ T-12；每列標註所屬 slice）

所有回應 JSON；錯誤 `{ "error": "CODE", "message": "..." }`。沿用現有 auth middleware。

| Method | Path | Body / Query | 回應 | Slice |
|---|---|---|---|---|
| GET | `/students` | | `[{id,name,grade}]` | `students` |
| POST | `/students` | `{name, grade}` | `{id,...}` | `students` |
| GET | `/subjects` | | `[{id,name,book_title,units:[{id,title,total,mastered}]}]`（mastered 需 `?student_id=`） | `catalog` |
| POST | `/sessions` | `{student_id, mode, subject_id, unit_id?, limit?}` | `SessionView` | `start_session` |
| GET | `/sessions/:id` | | `SessionView` | `session_nav` |
| POST | `/sessions/:id/answer` | `{question_id, kind:"choice"\|"text"\|"handwriting", choice?:string\|int, text?, image_png_base64?}` | `TutorReply` | `answer` |
| POST | `/sessions/:id/self_grade` | `{question_id, correct: bool}` | `TutorReply` | `answer` |
| POST | `/sessions/:id/message` | `{question_id, text}` | `TutorReply` | `ask_tutor` |
| POST | `/sessions/:id/next` | | `SessionView`（題目結束才允許，否則 409） | `session_nav` |
| POST | `/sessions/:id/end` | | `{answered, first_try_correct, weak_concepts:[...]}` | `session_nav` |
| GET | `/students/:id/report` | | `{weak_concepts, due_count, recent_wrong:[{question, attempts:[{verdict, input_image_url,...}]}]}` | `report` |
| GET | `/assets/*path` | | 靜態檔（`tutor_data/assets` 與 `tutor_data/answers`，**防 path traversal**） | `assets` |
| POST | `/admin/import` | `{root?}` | `{subjects, units, questions}` 計數 | `bank_import` |
| GET/PATCH | `/admin/questions/:id` | `{book_page?, book_where?, concept_tags?, active?}` | Question（含答案） | `bank_admin` |
| POST | `/admin/enrich` | `{subject_id}` | `{job_started:true}`（背景執行，T-17） | `bank_enrich` |
| GET | `/admin/ai/backends` | | `{available:[{id, version?, healthy, capabilities, model?}], active:["agy",...], activeVision:[...]}` | `ai_admin` |
| PUT | `/admin/ai/backends` | `{active:["agy","claude"], activeVision?:[...]}` | 同上；寫入 `settings` 並重建 registry；未知或不健康的 id 回 400 | `ai_admin` |
| POST | `/admin/ai/test` | `{backend_id, with_image: bool}` | 用固定測試題跑一次 `call_turn`，回 `{ok, durationMs, turn?, error?}`（管理頁「測試連線」按鈕） | `ai_admin` |

```ts
// frontend-vue/src/tutor/shared/types.ts（與後端 tutor/shared/dto.rs 欄位一致，serde rename_all = "camelCase"）
interface QuestionView {           // 不含答案
  id: string; qtype: string; inputMode: 'choice'|'char_grid'|'free_canvas'|'image_canvas'|'text';
  section: string; stem: string; options: string[]; charCount?: number;
  imageUrl?: string; bookPage?: number; bookWhere?: string;
}
interface SessionView {
  sessionId: string; mode: string; index: number; total: number;   // index 從 1 起
  qState: 'asking'|'hinting'|'revealed'|'explained';
  question?: QuestionView; empty?: boolean; message?: string;
  dialog: {role:'tutor'|'student'; content: string; bookRef?: BookRef}[];
}
interface BookRef { page: number; where: string }
interface TutorReply {
  verdict: 'correct'|'partial'|'incorrect'|'unreadable'|'none';
  say: string; action: string; bookRef?: BookRef;
  recognizedText?: string; followupQuestion?: string;
  qState: SessionView['qState'];
  reveal?: { answer: string; explanation: string };   // 已轉成顯示文字（選擇題顯示選項文字）
  canNext: boolean; selfGradeAvailable?: boolean;
}
```

`/answer`（`features/answer.rs`）處理順序：鎖 session → `store::load_session` → 驗證 `question_id` 為目前題目且 `q_state ∈ {asking, hinting}` → 手寫圖 base64 解碼存 `answers/<attempt_id>.png`（限制 ≤ 3MB、必須是 PNG magic bytes）→ 規則批改（`domain::question::grade_choice`）或 AI 批改（`ai::turn::call_turn`）→ `session.apply_verdict()` 得 `Step` →（需要時）AI 教學 → 單一 transaction 寫 `attempts` / `dialog_turns` / `sessions`，若 `Step.outcome` 有值再寫 `mastery`/`concept_weakness`（`domain::progress`）→ 回 `TutorReply`。

---

## 8. 前端（T-13 ~ T-16）

設計延續 jamie-exam：**遠距大字護眼**（題目 ≥ 32px、選項 ≥ 26px、米白暖色主題＋深色主題），全部操作有快捷鍵。Tutor 頁面獨立於現有 Console 的暗色 cyan 風格。

### TutorSession.vue 版面（橫向電視 16:9）

```
┌──────────────────────────────────────────────────────────────────────┐
│ 社會｜第 1 回｜第 3 / 15 題                        [我不懂 ?] [結束] │
├──────────────────────────────┬───────────────────────────────────────┤
│ QuestionCard                 │ TutorBubble（AI 老師對話，最新在下）    │
│  題幹（大字）                  │  🧑‍🏫 差一點點！翻到課本第 3 頁…      │
│  [插圖縮圖，可點開]            │                                       │
│ BookRefCallout（醒目黃色框）   │                                       │
│  📖 請翻到 第 3 頁 右上角的地圖 │                                       │
├──────────────────────────────┴───────────────────────────────────────┤
│ AnswerPanel：依 inputMode                                              │
│   choice      → 大按鈕（O/X 或 1~4），鍵盤 1-4 / O / X                  │
│   char_grid   → HandwritingPad 米字格 × charCount                       │
│   free_canvas → HandwritingPad 自由書寫（多行、可加頁）                  │
│   image_canvas→ HandwritingPad 以題目插圖為底圖                         │
│   text        → 大輸入框                                               │
│ [清除 C] [復原 Z] [送出 Enter]        AI 思考中 → 顯示「老師想一下…」動畫 │
└──────────────────────────────────────────────────────────────────────┘
```

### HandwritingPad.vue（`features/tutor-session/`，T-14）

- 移植 `C:\workspace\jamie-exam\web\canvas.js`（筆刷/橡皮擦/復原/米字格/底圖），改寫成 Vue SFC + TypeScript。
- Pointer Events，支援壓感（`pressure`），`touch-action: none`。
- `exportPng(): string`：
  - 白底、黑色筆跡；`char_grid` **不輸出**格線（避免干擾辨識）但字與字之間保留間距；
  - `image_canvas` 輸出「底圖＋筆跡」合成圖；
  - 最長邊縮到 1024px；回傳 `data:image/png;base64,...` 去掉前綴後送出。
- 空白畫布不允許送出。

### 其他頁面

- `StudentPicker.vue`：大頭貼式按鈕選學童，可新增。
- `SessionSetup.vue`：選科目 → 選模式（📖 循序 / 🎯 弱點複習（顯示待複習數）/ 🔀 混合）→ 選單元（循序時）。
- `ParentReport.vue`：弱點概念排行、待複習數、最近錯題（含學童手寫原圖、AI 辨識結果、AI 的 misconception）。
- `QuestionAdmin.vue`：表格列出題目，可編輯 `book_page`、`book_where`、`concept_tags`、`active`；可點開掃描頁對照。
  另有「AI 後端」分頁：列出偵測到的 CLI（版本、是否支援看圖）、拖曳排序主清單與看圖清單、「測試連線」按鈕。

---

## 9. 設定（環境變數）

| 變數 | 預設 | 說明 |
|---|---|---|
| `TUTOR_DATA_DIR` | `./tutor_data` | 執行期資料根目錄 |
| `TUTOR_IMPORT_ROOT` | `C:\workspace\jamie-exam` | 匯入來源 |
| `TUTOR_AI_BACKENDS` | `agy` | 主後端清單（逗號分隔，依序容錯）；DB `settings` 有值時以 DB 為準 |
| `TUTOR_AI_BACKENDS_VISION` | （同主清單） | 看圖用途的後端清單 |
| `TUTOR_MODEL_<ID>` | （空，用 CLI 預設） | 各後端 model，例 `TUTOR_MODEL_AGY`、`TUTOR_MODEL_CLAUDE` |
| `TUTOR_AI_BACKENDS_FILE` | `<TUTOR_DATA_DIR>/ai_backends.toml` | GenericAdapter 設定檔 |
| `TUTOR_AI_TIMEOUT_SECS` | `90` | |
| `TUTOR_AI_CONCURRENCY` | `2` | |
| `ANTIGRAVITY_CLI_BIN` | 自動偵測 | 沿用既有，agy adapter 的 program |
| `TUTOR_CLI_BIN_<ID>` | 各 adapter 預設 | 覆寫某後端的執行檔路徑，例 `TUTOR_CLI_BIN_CLAUDE`；測試時指向 fake CLI（由 `tutor/ai/backends.rs` 讀取後放進 `BackendConfig`） |

---

## 10. 工單（依序執行）

> 每張工單完成條件：`cargo build` 無 warning 新增、`cargo test` 通過、前端 `pnpm build` 通過。不要修改工單範圍外的檔案。

### Phase 0 — 可行性驗證（**必須最先做，結果決定後續**）

**T-00　agy vision 能力探測**（相依：無）
- 在 scratch 目錄建立 `runs/probe/`，放一張手寫「東北」的 PNG（可用任何繪圖工具或 Python PIL 畫字）與 `context.json`。
- 依 §6.1 方式呼叫 agy，prompt 要求「讀 ./answer.png，辨識文字，寫入 ./out/turn.json」。
- 依序嘗試：(a) prompt 中寫 `./answer.png`；(b) `@answer.png` 引用語法；(c) 查 `agy --help` 是否有 `--image`/`--attach` 類參數。
- 記錄：哪種方式成功、使用模型、平均延遲（跑 5 次）、token 量。
- 若本機也裝了 `claude` / `gemini` / `codex`，用相同圖片各跑一次（指令見 §6.1），順便核對 §6.1 表格中的旗標，並各存一份 stdout 樣本到 `backend-rust/crates/ai-cli/tests/fixtures/<id>_stream.txt`（T-07/T-08 要用）。
- **產出**：在本文件末尾「附錄 A：探測結果」填寫結果（每個 CLI 一列）。若 agy 三種方式皆失敗 → agy 的 `vision=None`；若有其他 CLI 可看圖，建議將它設為 `TUTOR_AI_BACKENDS_VISION`。

### Phase 1 — 核心域與資料層（不含 AI）

**T-01　DB**（相依：無）
- `tutor/shared/migrations/0001_init.sql`（§3 原文）、`tutor/shared/db.rs`：`pub async fn open(data_dir: &Path) -> SqlitePool`（建立目錄、WAL、foreign_keys、執行 schema）。
- 驗收：以 `:memory:` 開啟，查 `sqlite_master` 確認 11 張表。

**T-02　domain/question**（相依：無）
- `Question`、`QType`、`InputMode`、`Answer`（值物件：`TrueFalse(bool)` / `Choice(usize)` / `Text(String)` / `Math{final, steps_hint}`）、`Verdict`、`BookRef`。
- 純函式：`grade_choice(&Question, &ChoiceInput) -> Verdict`（`true_false` 接受 `"O"/"o"/"○"`、`"X"/"x"/"×"`；`single_choice` 接受 0-based int 或選項文字）、`answer_display(&Question) -> String`、`contains_answer(&Question, text) -> bool`（洩答檢查）、`normalize_import(raw) -> Result<Question>`（§4.1 轉換：1-based → 0-based、`input_mode` 推導、頁碼推導）。
- 驗收：上述每個變體一個單元測試；`normalize_import` 以 `soc_01_mc_1`、`soc_01_map_1` 兩筆真實資料測試。

**T-03　domain/progress**（相依：無）
- `Mastery`、`ConceptWeakness`、`Outcome(FirstTry|AfterHint|Failed)`；`next_mastery(prev: Option<Mastery>, outcome, now) -> Mastery`、`update_weakness(prev: Option<ConceptWeakness>, outcome, note, now) -> ConceptWeakness`。
- 驗收：§5.1 表格每列一個單元測試。

**T-04　domain/session**（相依：T-02, T-03）
- `Session` 聚合（欄位對應 `sessions` 表）、`QState`、`TeachAction`、`Step`；方法：`current_question_id()`、`try_no()`、`apply_verdict(v, now) -> Step`、`can_next()`、`advance()`、`finish(now)`。
- `select_queue(mode, candidates: &Candidates, n, seed) -> Vec<String>`（§5.2；`Candidates` 為 slice 查好的資料）。
- 驗收（純單元測試，無 DB）：
  1. 第一次答對 → `Step{action: Praise, reveal: true, outcome: Some(FirstTry)}`。
  2. 錯兩次 → 第 1 次 `Hint`、無 reveal；第 2 次 `Explain`、reveal、`requeued=true`；同題第二輪再錯兩次 → `requeued=false`。
  3. `Unreadable` → `try_no` 不變、`RetryUnreadable`。
  4. `select_queue` 的 `review` 在候選為空時回空；`progress` 保持順序；`review` 同 seed 結果固定。

**T-05　store + shared**（相依：T-01~T-04）
- `tutor/store.rs`：`load_session/save_session`、`load_progress(student_id, question_id)/save_progress`、`load_question`、`load_weaknesses(student_id, top)`；全部接受 `&mut SqliteConnection`（可在 transaction 內用）。
- `tutor/shared/dto.rs`（§7 的 view 型別＋`From<&Question> for QuestionView`，**不含答案**）、`tutor/shared/error.rs`。
- 驗收：`:memory:` 往返測試（save → load 相等）。

**T-06　slice：bank_import**（相依：T-05）
- `tutor/features/bank_import.rs`：依 §4.2；`POST /admin/import` 與 `pub async fn import_all(pool, root, data_dir) -> ImportStats`（啟動時若題庫為空自動呼叫一次）。
- 驗收：對真實 `questions.json` 匯入後 `questions` = 54 筆；`soc_01_mc_1` 的 `answer_json` = `1`；`soc_01_map_1` 的 `book_page` = 3、`input_mode` = `image_canvas`；重跑一次筆數不變。

### Phase 2 — AI 引擎與防腐層

**T-07　crates/ai-cli 核心 + agy adapter**（相依：T-00）
- 建立 `backend-rust/crates/ai-cli`（library crate），`backend-rust/Cargo.toml` 加 path 依賴。檔案見 §2，型別見 §6.0。
- 把 `executor/runner.rs` 第 45~58 行的 agy 偵測搬到 `adapters/agy.rs::resolve_agy_bin()`，`runner.rs` 改呼叫它（**行為不得改變**，runner 其餘不動）。`cli.rs` 提供 `which(program) -> bool`（Windows 用 `where.exe`）。
- `AgyAdapter` 的 `vision` 依 T-00 結果設定。
- `MockEngine`：建構時給一串 `Result<AiRawResult, AiError>`，每次 `run` 依序吐出，並記錄收到的 `AiInvocation`。
- `tests/fake_cli/`：小程式或 `.cmd`/`.sh`，讀 `FAKE_CLI_MODE`（`ok`/`badjson`/`timeout`/`exit1`），在 cwd 寫 `out/turn.json` 並輸出 agy 格式 NDJSON。
- **crate 內不得出現**：sqlx、axum、任何 tutor 型別或中文教學文字。
- 驗收：
  1. `AgyAdapter::build_command` args 快照；`parse_line` 對 `tests/fixtures/agy_stream.txt`。
  2. `CliEngine<AgyAdapter>` + fake CLI：四種模式各一個整合測試。
  3. `call_json` + `MockEngine`：(a) 第一個 `Timeout`、第二個成功 → 回第二個結果、`CallRecord` 2 筆；(b) 解析失敗同 engine 重試 1 次；(c) 需要圖片時 `vision=None` 的 engine 被跳過。
  4. `cargo build` 後 Console 呼叫 agy 的行為與改動前一致（手動跑一個既有任務）。

**T-08　crates/ai-cli 其他 adapter**（相依：T-07）
- `adapters/claude_code.rs`、`gemini_cli.rs`、`codex.rs`、`generic.rs`（`ai_backends.toml` 解析）。
- 先跑各 CLI `--help` 核對 §6.1 旗標，差異更新回本文件。
- 驗收：每個 adapter 有 `build_command` 快照與 `parse_line` fixture 測試（fixture 來自 T-00；沒裝該 CLI 就依文件手寫樣本並註明「未實測」）；`generic` 測 `{model_args}` 為空時整組移除、`{image_args}` 每張圖展開。
- 本工單**只能**修改 `crates/ai-cli/` 內的檔案。

**T-09　tutor/ai 防腐層**（相依：T-05, T-07）
- `prompts.rs`：`build_prompt(purpose, ctx, caps) -> String`、`build_context_json(...)`，模板依 §6.3；`image_ref` / `output_instruction` / `context_instruction` 依能力表。
- `turn.rs`：`AiTurn`（§6.2）、`TutorFeedback`、`call_turn()`（包 `ai_cli::call_json`，寫 `ai_calls`，洩答/頁碼 guard）、罐頭回覆（§6.4）。
- `backends.rs`：讀 settings/環境變數 → `BackendConfig` → `ai_cli::Registry`；`engines_for(purpose)`；`reload()`。
- 驗收：
  1. 每種 purpose 的 prompt 測試：含 System 與任務區塊；無殘留 `{placeholder}`；`reads_files=false` 時內含 context JSON；`vision=AtMention` 時圖片寫成 `@answer.png`。
  2. `AiTurn` 反序列化（合法、缺欄位、未知 enum 值）。
  3. `call_turn` + `MockEngine`：AI 的 hint 內含答案字串 → `say` 被換成罐頭提示；AI 給錯頁碼 → 改為題目 `book_page`；`ai_calls` 有紀錄。

### Phase 3 — 後端 slices

> 每個 slice 一個檔、匯出 `routes()`；slice 間不互相 import（§1.1）。

**T-10　基礎 slices＋組裝**（相依：T-05, T-09）
- `students.rs`、`catalog.rs`、`assets.rs`（`canonicalize` 後必須仍在 `tutor_data/assets` 或 `tutor_data/answers` 底下）。
- `tutor/mod.rs`：`TutorState { pool, data_dir, ai: Arc<Backends>, ai_sem, session_locks }`、`init()`（開 DB、題庫為空時匯入、建 registry、health check、清 `runs/`）、`routes()` 合併所有 slice；`main.rs` 掛到 `/api/tutor`（沿用 auth middleware）。
- 驗收：`curl` 建學童、列科目、取一張插圖；`/assets/../tutor.db` 回 403/404。

**T-11　教學 slices**（相依：T-10）
- `start_session.rs`、`session_nav.rs`、`answer.rs`、`ask_tutor.rs`，依 §5、§7。
- 驗收（`:memory:` + `MockEngine`）：
  1. 選擇題答對 → 不呼叫 AI、`qState=revealed`、mastery box=1。
  2. 選擇題錯兩次 → 第 1 次 `hinting` 無 `reveal`；第 2 次 `explained` 有 `reveal`；queue 尾端多一題。
  3. 手寫題 `unreadable` → `q_try` 不增加。
  4. 看圖清單為空時手寫題 → 不呼叫 AI，回 `selfGradeAvailable=true`。
  5. 同 session 並行兩個 `/answer` → 其一 409。

**T-12　管理與報表 slices**（相依：T-10）
- `report.rs`、`bank_admin.rs`、`ai_admin.rs`，依 §7。
- 驗收：`curl` 腳本跑完「建學童 → 開 session → 答題 → next → end → report」；`PUT /admin/ai/backends` 帶未知 id 回 400。

### Phase 3b — 前端

**T-13　路由與共用層**（相依：T-10）
- 安裝 `vue-router`；`/` 保持現有 Console（不搬動既有檔案），新增 `/tutor`、`/tutor/session/:id`、`/tutor/report/:studentId`、`/tutor/admin`。
- `tutor/shared/http.ts`（fetch 包裝，沿用現有 AuthFetch）、`tutor/shared/types.ts`（§7）、`BigButton.vue`、`theme.css`（米白暖色／深色護眼主題）。

**T-14　HandwritingPad**（相依：無，可與 T-13 並行）
- 位於 `tutor/features/tutor-session/HandwritingPad.vue`，依 §8 規格。驗收：在平板/手寫板上能畫、清除、復原；`exportPng` 最長邊 ≤ 1024；`char_grid` 匯出無格線。

**T-15　feature：tutor-session**（相依：T-11, T-13, T-14）
- `TutorSession.vue`、`useTutorSession.ts`（狀態與 API 呼叫）、`api.ts`、`QuestionCard`、`BookRefCallout`（book_ref 出現時閃爍提示 2 秒）、`AnswerPanel`、`TutorBubble`。
- 快捷鍵：`Enter` 送出/下一題、`C` 清除、`Z` 復原、`1-4`/`O`/`X` 作答、`?` 開「我不懂」輸入框。
- AI 呼叫中鎖定送出按鈕並顯示「老師想一下…」；`409` 時忽略。手寫題 `selfGradeAvailable` 時顯示標準答案疊圖與 ✅/❌。

**T-16　features：start / parent-report / admin**（相依：T-12, T-13）
- `start/`（StudentPicker＋SessionSetup）、`parent-report/`、`admin/`（QuestionAdmin＋AiBackends），依 §8。

### Phase 4 — 題庫補強與新科目

**T-17　slice：bank_enrich（書本頁碼/位置 AI 補強）**（相依：T-09, T-12）
- `tutor/features/bank_enrich.rs`：背景工作，對某科目每張掃描頁（檔名不含「解答」）呼叫 `ai_cli::call_json::<Vec<EnrichItem>>(backends.engines_for(EnrichPage), …)`；檔案放 `page.jpg` + `questions.json`（該科目 `book_page` 為 NULL 或 `book_where` 為空的題目，只含 id/stem/options）。enrich 專用 prompt 寫在本 slice 內（只有它用，不放 `ai/prompts.rs`）。
- 任務 prompt：「找出哪些題目印在這一頁，以及在頁面的哪個位置（例如『左上第二大題』『右下角的地圖』），輸出 JSON：`[{"id":"...","where":"...","concept_tags":["..."]}]`」。
- 只寫入 NULL/空欄位（不覆寫人工值），`book_page` = 掃描編號 + `page_offset`。
- 驗收：社會科跑完後 `book_page IS NULL` 的題數下降；人工抽查 5 題位置描述正確。

**T-18　新科目匯入管線（國文/數學）**（相依：T-17）
- 掃描放 `jamie-exam/data/scans/{國文|數學}/`，題目頁＋解答頁成對交給看圖用 AI 後端萃取成 §4 的題目 JSON（新增 `math_expr`、`chinese_write`），輸出 `questions_{subject}.json` 後走 T-06 匯入。實作為 `bank_import.rs` 的擴充或新 slice `bank_extract.rs`（> 80 行就新開）。
- 萃取結果**需人工在 QuestionAdmin 審核**（`source='ai_extract'`，萃取時預設 `active=0`，審核後改 `active=1`）。

### Phase 5 — 可選加值

**T-19　語音**：`speechSynthesis`（`zh-TW`）朗讀 `say`；Chrome 下用 `webkitSpeechRecognition` 讓學童口說回答 `text`/`student_message`。設定頁可關閉。放在 `tutor-session` feature 內。
**T-20　家長通知**：session 結束摘要可匯出 / 列印（`parent-report` feature）。
**T-21　HttpApiEngine（預留）**：在 `crates/ai-cli` 新增直接呼叫模型 HTTP API 的 `AiEngine` 實作（圖片以 base64 內嵌，`reads_files/writes_files=false`），免 spawn、延遲較低。只改 crate＋registry 註冊一行。
**T-22　Console 改用 ai-cli（可選）**：讓 `executor/runner.rs` 透過 `ai_cli` 的 `CliAdapter::build_command/parse_line` 組指令與解析串流，Console 即可切換不同 CLI。需另外處理 runner 的串流 log 與 git diff，獨立評估。

---

## 11. 測試策略

- **domain/**：純單元測試，無 DB、無 AI、無時間/亂數依賴（`now`、`seed` 由參數傳入）。這是測試密度最高的地方。
- **slices**：SQLite `:memory:` + `MockEngine` 的整合測試，直接呼叫 handler 或用 `tower::ServiceExt::oneshot`。
- **crates/ai-cli**：`build_command`、`parse_line` 純函式測試 + fixture；行程管理用 fake CLI（`TUTOR_CLI_BIN_<ID>`）。CI 不呼叫真 CLI。
- **真 CLI 冒煙測試**：`cargo test -- --ignored real_cli_<id>`，手動跑；用 T-00 的探測圖片。
- **前端**：手動驗收清單——遠距 2.5 公尺可讀、手寫板延遲無明顯拖影、AI 等待期間不可重複送出。

## 12. 風險與對策

| 風險 | 對策 |
|---|---|
| agy 無法在 `-p` 模式讀圖 | T-00 先驗證；可把看圖用途指派給其他 CLI（`TUTOR_AI_BACKENDS_VISION`）；都不行則手寫題退回「自評」 |
| 各家 CLI 改版、旗標或輸出格式變動 | 差異集中在單一 adapter 檔案＋fixture 測試；`GenericAdapter` 可不改程式碼調整；多後端清單自動容錯 |
| 各家輸出品質不同（JSON 遵從度、中文用語） | `call_turn` 嚴格 schema 驗證＋重試；`ai_calls` 記錄 backend，可在管理頁比較各後端失敗率 |
| 每次 spawn CLI 延遲高（數秒~數十秒） | 選擇題答對不叫 AI；手寫批改與教學合併成一次呼叫；UI 等待動畫 |
| AI 洩漏答案 / 亂給頁碼 | engine 覆寫 action、字串檢查答案、頁碼白名單 |
| AI 輸出非 JSON | 檔案輸出優先、擷取 `{...}`、重試 1 次、罐頭回覆 |
| 手寫辨識錯誤導致誤判 | 顯示 `recognizedText`（「老師看到你寫的是：東比」），提供「老師看錯了」按鈕 → `self_grade` |
| 掃描編號 ≠ 印刷頁碼 | `subjects.page_offset`，在 QuestionAdmin 可調 |
| `--dangerously-skip-permissions` 寫到不該寫的地方 | cwd 限縮為 `runs/<id>/`，只放必要檔案；不放 DB 與其他學童資料 |

---

## 附錄 A：T-00 探測結果

（由執行 T-00 的 agent 填寫）

| CLI | 版本 | 圖片引用方式（PromptPath / AtMention / Flag / 不支援） | 能寫 out/turn.json | 模型 | 平均延遲 | 平均 token | 與 §6.1 旗標差異 |
|---|---|---|---|---|---|---|---|
| agy | 1.3.5 | PromptPath (view_file / python) | ✅ (工具寫入) | 預設 | ~34s | ~88k (含 agent 迴圈) | 無差異，Windows 需注意單雙引號避免轉義問題 |
| claude | 2.1.296 | PromptPath (文件/工具模式) | 未登入 (Not logged in) | opus-5-5 | - | - | 無差異 |
| gemini | 0.3.4 | AtMention (文件模式) | 未設定 API Key | 預設 | - | - | 無差異 |
| codex | 未安裝 | Flag{"-i"} | - | - | - | - | 未安裝 |

# 改善計畫（Improvement Plan）

> 原本的 `plan.md`（AI 家教系統設計書）已移到 [`doc/tutor-design.md`](doc/tutor-design.md)。
> 每一步完成後各自在 `main` 上 commit。勾選代表已完成。

## Step 1 — 安全：綁定位址與 Tutor 管理端點驗證
- [x] `HOST` 預設改為 `127.0.0.1`。
- [x] 綁定非本機位址且 `ANTIGRAVITY_API_KEY` 為空時拒絕啟動（防止內網無驗證曝露；可用 `ANTIGRAVITY_ALLOW_INSECURE=1` 明確略過）。
- [x] `/api/tutor/admin/*` 套用 `require_api_key`（前端 `tutorFetch` 已會帶上 Console 設定的 API Key）。

## Step 2 — 安全：CORS 與 Docker 設定
- [x] CORS 由 `Any` 改為白名單（`CORS_ORIGINS`，預設 `http://localhost:5173`、`http://127.0.0.1:5173`）；前端預設改走同源（Vite proxy / nginx），WebSocket 由 `location.origin` 推導。
- [x] `docker-compose.yml` 改從 `.env` 讀取 API Key，新增 `.env.example`。

## Step 3 — 啟動流程
- [x] `tutor::init` 從 `create_router` 的 `block_in_place` 移到 `main` 的 async 流程，初始化失敗即終止。
- [x] `TUTOR_DATA_DIR` 解析為絕對路徑並在啟動時印出。

## Step 4 — 健壯性與可維護性
- [x] 清除請求路徑上的 `unwrap()`／`expect()`（列舉轉字串改用 `shared::enum_tag`）。
- [x] 拆分 `tutor/features/answer.rs`：寫入移到 `store.rs`（`insert_attempt`／`insert_dialog_turn`／`record_outcome`），上下文組裝移到 `ai/context.rs`（與 `ask_tutor` 共用），base64 改用 `base64` crate；自評也加上 session 鎖、狀態檢查與弱點更新。

## Step 5 — 測試
- [x] 後端：以 `MockEngine` + SQLite 的 HTTP 端到端測試（開始 → 作答 → 提示 → 揭曉；確認揭曉前答案不外洩；admin 端點需要 key）。過程中抓到並修正 `BookRef` 欄位名前後端不一致（`whereClause` vs `where`）。
- [x] 前端：導入 Vitest，為 `useTutorSession` 等邏輯加入單元測試。

## Step 6 — CI
- [x] GitHub Actions：`cargo test`、`cargo clippy -D warnings`、`pnpm build`、`pnpm test`（先以 `clippy --fix` 清掉既有警告；Docker 映像升到 rust 1.90，因 let-chains 需 1.88+；需真實題庫的測試在找不到素材時跳過）。

## Step 7 — 文件與目錄整理
- [x] 更新 README（實際目錄、Tutor 系統、啟動方式、環境變數）。
- [x] 題庫 Python 腳本移到 `tools/bank-pipeline/`，路徑改由參數／環境變數提供，附 README。
- [x] `doc/design*.md` 依主題重新命名並加上 `doc/README.md` 索引（空白的 `design5.md` 已刪除）。

## Step 8 — Console 改用 ai-cli（T-22）
- [x] `executor/runner.rs` 透過 `ai_cli` 的 `AgyAdapter::build_command` 組 CLI 指令，移除重複的旗標組裝；`AiInvocation` 新增 `extra_args`（Console 的 extraFlags），各 adapter 放在 prompt 之前（generic 可用 `{extra_args}` 指定位置）。
- 串流解析仍保留 Console 自己的版本：agy 會把 `usage` 與 `text_delta` 放在同一行，而 Console 的 `TokenUsage` 含 thinking/cache tokens，改用 `parse_line`（一行只回一個項目）會遺失用量資訊。要統一需先讓 `parse_line` 能回傳多個項目，另案處理。

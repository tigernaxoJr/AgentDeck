# Antigravity Web-CLI Driver

以 Web 介面驅動 AI CLI（預設 Antigravity `agy`）的單一 Rust 服務，包含兩個子系統：

- **Tutor**（`/tutor`）：AI 家教複習系統。學童拿著實體課本作答，是非/選擇題由規則即時批改，答錯時 AI 給課本頁碼提示，手寫題交給具 vision 能力的 CLI 批改，並以間隔複習記住不熟的題目與概念。
- **Console**（`/console`）：派發 prompt 給 CLI、即時串流輸出、檢視與還原 Git 變更、一鍵 Commit。

設計文件見 [`doc/`](doc/README.md)，目前的改善計畫見 [`plan.md`](plan.md)。

---

## 專案結構

```
cli_as_driver/
├── backend/                 # Rust 2024（Axum + Tokio + sqlx/SQLite WAL）
│   ├── crates/ai-cli/       # 獨立 crate：AiEngine / CliAdapter（agy、claude、gemini、codex、TOML 自訂）、MockEngine
│   └── src/
│       ├── main.rs          # 啟動、綁定位址檢查、三階段 Graceful Shutdown
│       ├── api/             # Console REST + WebSocket、API Key 中介軟體
│       ├── executor/        # TaskRunner / TaskScheduler / Git 快照
│       ├── storage/         # Console 任務 DB（antigravity_tasks.db）與版本化遷移
│       └── tutor/           # VSA + DDD：features/（一個用例一檔）、domain/、store.rs、ai/（防腐層）、shared/
├── frontend/                # Vue 3 + TypeScript + Tailwind 4 + Pinia + Xterm.js
│   └── src/
│       ├── components/      # Console
│       └── tutor/features/  # start、tutor-session（含手寫板）、parent-report、admin
├── tools/bank-pipeline/     # 從掃描頁產生題庫的 Python 腳本（見其 README）
├── docker/ Dockerfile docker-compose.yml
└── start.ps1 / start.sh     # 一次啟動前後端
```

---

## 開發

需求：Rust 1.88+、Node 24 + pnpm 10、已登入的 `agy`（或其他支援的 CLI）。

```powershell
./start.ps1          # 或 ./start.sh；分別啟動 backend (cargo run) 與 frontend (pnpm dev)
```

- 前端：<http://localhost:5173>（`/` 會導向 `/tutor`，Console 在 `/console`，後台在 `/tutor/admin`）
- 後端：<http://127.0.0.1:3000>；前端透過 Vite proxy 以同源方式存取 `/api`

API Key：設定 `ANTIGRAVITY_API_KEY` 後，Console 的所有 API 與 Tutor 的 `/api/tutor/admin/*` 都需要驗證；
在 Console 頁面左側的「API Key」欄位輸入同一組金鑰（存在瀏覽器 localStorage，Tutor 後台共用）。學童端 API 不需金鑰。

### 測試

```powershell
cd backend;  cargo test -p backend -p ai-cli; cargo clippy --all-targets -- -D warnings
cd frontend; pnpm test; pnpm build
```

後端測試不呼叫真實 CLI（以 `MockEngine` 取代）；CI 設定在 `.github/workflows/ci.yml`。

---

## 環境變數

| 變數 | 預設 | 說明 |
|------|------|------|
| `HOST` / `PORT` | `127.0.0.1` / `3000` | 綁定位址。非本機位址且未設 API Key 時拒絕啟動 |
| `ANTIGRAVITY_API_KEY` | （空＝開發模式免驗證） | 存取金鑰 |
| `ANTIGRAVITY_ALLOW_INSECURE` | — | 設為 `1` 才允許無金鑰對外綁定（不建議） |
| `CORS_ORIGINS` | `http://localhost:5173,http://127.0.0.1:5173` | 跨域白名單（僅在前端直連後端時需要） |
| `ANTIGRAVITY_WORKSPACE` | 目前目錄 | Console 允許 CLI 操作的根目錄 |
| `ANTIGRAVITY_CLI_BIN` | 自動偵測 `agy` / `antigravity` | Console 使用的 CLI |
| `DATABASE_URL` | `sqlite://antigravity_tasks.db` | Console 任務資料庫 |
| `TUTOR_DATA_DIR` | `./tutor_data` | Tutor 資料目錄（`tutor.db`、素材、手寫答案），啟動時印出絕對路徑 |
| `TUTOR_IMPORT_ROOT` | `C:/workspace/jamie-exam` | 題庫為空時自動匯入的素材根目錄 |
| `TUTOR_AI_BACKENDS` / `TUTOR_AI_BACKENDS_VISION` | `agy` | 啟用的 AI 後端（逗號分隔，可在後台改） |
| `TUTOR_CLI_BIN_<ID>` / `TUTOR_MODEL_<ID>` | — | 覆寫某後端的執行檔或模型，例 `TUTOR_CLI_BIN_CLAUDE` |
| `TUTOR_AI_CONCURRENCY` | `2` | 同時進行的 AI 呼叫數 |

前端建置時可設 `VITE_API_BASE` 直連後端（需同時加入 `CORS_ORIGINS`）。

---

## Docker

```bash
cp .env.example .env   # 填入 ANTIGRAVITY_API_KEY
docker compose up -d --build
```

服務在 <http://localhost:8080>，nginx 託管前端並把 `/api` 代理到容器內的後端。未設定 `ANTIGRAVITY_API_KEY` 時 compose 會拒絕啟動。

# Antigravity Web-CLI Console System

依據 `doc/` 文件規格建構之「Web 介面驅動並管理 Antigravity CLI」系統。

---

## 專案結構

```
cli_as_driver/
├── backend-rust/       # Rust (Axum + Tokio + SQLite WAL + Git Inspector)
│   ├── Cargo.toml
│   └── src/
│       ├── api/        # REST 路由 (CRUD, Diff, Revert, Commit) + WebSocket 智慧訂閱 + Auth Middleware
│       ├── executor/   # TaskRunner (50ms 視窗緩衝, Headless 模式), TaskScheduler (Semaphore 並行, 優雅關閉), Git
│       ├── models/     # Tagged Union 通訊協定 (protocol.rs) 與單元測試
│       ├── storage/    # SQLite WAL 持久化與孤兒任務修復 (db.rs)
│       └── main.rs     # 伺服器啟動入口與三階段 Graceful Shutdown
└── frontend-vue/       # Vue 3 + TypeScript + Tailwind CSS 4 + Pinia + Xterm.js
    ├── package.json
    ├── vite.config.ts
    └── src/
        ├── types/      # 與後端嚴格對齊之 Discriminated Unions 協定
        ├── stores/     # useTaskStore (Ring Buffer, useWebSocket 心跳/重連, AuthFetch)
        └── components/
            ├── AntigravityConsole.vue  # 主控制台 (左側佇列、頂部派發、Xterm 即時串流、Artifact 檢視)
            └── GitDiffViewer.vue       # 雙軌 Unified Diff 渲染、還原檔案、一鍵 Commit
```

---

## 啟動方式

### 1. 啟動後端服務 (Rust Axum)

```powershell
cd backend-rust
cargo run
```

預設監聽在 `http://127.0.0.1:3000`。
- 環境變數：
  - `ANTIGRAVITY_WORKSPACE`：允許 CLI 操作之根目錄（預設為當前目錄）
  - `DATABASE_URL`：SQLite 資料庫連線字串（預設為 `sqlite://antigravity_tasks.db`）
  - `ANTIGRAVITY_API_KEY`：可選安全驗證 Key（留空即為本地開發免認證模式）
  - `ANTIGRAVITY_CLI_BIN`：CLI 執行檔名稱或路徑（預設會自動偵測 `agy` 或 `antigravity`）

### 2. 啟動前端開發介面 (Vue 3 + Vite)

```powershell
cd frontend-vue
npm run dev
```

瀏覽器打開 `http://localhost:5173` 即可使用。

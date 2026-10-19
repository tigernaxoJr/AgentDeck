這份系統架構設計文件（System Architecture Document, SAD）針對「以 Web 介面驅動並管理 Antigravity CLI」的需求量身打造，採用 **Rust (Axum + Tokio)** 作為高併發非同步後端，搭配 **Vue 3 + TypeScript + Tailwind CSS** 建構響應式前端控制台。

---

## 1. 系統整體架構與技術棧 (System Overview & Tech Stack)

系統核心設計理念為「非同步行程代理與雙向狀態同步」。由 Rust 後端負責管理底層作業系統的子行程（Subprocess）生命週期、I/O 緩衝與並行限制，並透過 WebSocket 將終端機串流與結構化結果即時映射至 Vue 3 前端。

| 層級                      | 核心技術 / 框架                             | 關鍵套件 / 模組                           | 職責說明 |
| ------------------------- | ------------------------------------------- | ----------------------------------------- | -------- |
| **前端展示層 (Frontend)** | Vue 3 (Composition API) + TypeScript + Vite | • **Tailwind CSS**：UI 佈局與暗黑模式<br> |

<br>• **Pinia**：管理任務佇列與連線狀態<br>

<br>• **Xterm.js**：渲染 ANSI 終端機日誌<br>

<br>• **VueUse (`useWebSocket`)**：管理重連與心跳 | 提供指令下達表單、即時終端機輸出視窗、任務排程佇列面板，以及結構化產出（JSON / Markdown）的視覺化檢視器。 |
| **通訊協定層 (Protocol)** | RESTful API + WebSocket | • **HTTP JSON**：任務建立、歷史紀錄查詢、系統設定<br>

<br>• **WebSocket (WSS)**：即時雙向事件與 `stdout`/`stderr` 串流 | 解耦「控制指令（HTTP）」與「長時執行狀態回報（WS）」，防止長任務造成 HTTP Timeout，並支援斷線重連後狀態恢復。 |
| **後端核心層 (Backend)** | Rust (2024 Edition) | • **Axum**：Web 與 WebSocket 路由伺服器<br>

<br>• **Tokio**：非同步執行緒池與 `tokio::process`<br>

<br>• **Serde**：JSON 序列化與通訊協定定義<br>

<br>• **SQLx (SQLite)**：輕量級本地任務狀態持久化 | 接收前端指令、控管 CLI 並行數量（Semaphore）、攔截系統訊號、安全調用 CLI 子行程並解析輸出結果。 |
| **執行引擎層 (Execution)** | Antigravity CLI + OS | • **Dedicated Workspace**：隔離的工作目錄<br>

<br>• **Auth Context**：繼承宿主機 CLI 認證與 Token | 實際消耗綁定帳號之專屬額度（Token / Rate Limits），執行自主代理工作流並產出最終檔案與日誌。 |

---

## 2. 核心模組與專案目錄結構 (Module & Directory Structure)

### 後端架構 (`backend-rust/`)

採用職責分離的模組化設計，將 Web 路由與底層行程控制完全解耦：

- **`api/` (介面層)**
- `routes.rs`：定義 REST 端點（`POST /api/tasks`, `GET /api/tasks/:id`, `DELETE /api/tasks/:id/cancel`）。
- `ws.rs`：處理 WebSocket 握手與訂閱機制，透過 `tokio::sync::broadcast` 將特定任務的輸出廣播給訂閱的前端連線。

- **`executor/` (CLI 執行引擎層 — 核心模組)**
- `scheduler.rs`：使用 `tokio::sync::Semaphore` 實作並行控制器（例如限制同時最多執行 2 個 CLI 行程），超額任務自動進入記憶體佇列等待，避免觸發帳號 Rate Limit。
- `runner.rs`：封裝 `tokio::process::Command`。負責設定工作目錄（Working Directory）、注入環境變數、強制掛載非互動旗標，並透過 `BufReader` 非同步逐行讀取 `stdout` 與 `stderr`。
- `watchdog.rs`：超時監控與強制終止機制（透過 `Child::kill` 或發送 `SIGTERM`），防止代理陷入無窮迴圈耗盡 Token。

- **`models/` (資料模型層)**
- `task.rs`：定義任務狀態機（`Queued` -> `Running` -> `Completed` / `Failed` / `Cancelled`）。
- `protocol.rs`：定義前後端 WebSocket 溝通的強型別結構（Tagged Enum）。

- **`storage/` (持久化層)**
- `db.rs`：使用 SQLite 紀錄每次任務的 Prompt、消耗時間、執行狀態與產出檔案路徑，確保網頁重新整理後歷史紀錄不遺失。

### 前端架構 (`frontend-vue/`)

採用 Feature-based 結構，確保 TypeScript 型別安全與狀態集中管理：

- **`src/types/protocol.ts`**：與後端 Rust `serde` 完全對齊的 TypeScript 介面定義（Discriminated Unions）。
- **`src/stores/useTaskStore.ts` (Pinia)**：維護當前所有任務列表、活躍中的 WebSocket 實例，以及當前選中任務的即時 Log 緩衝區（Ring Buffer，防止超長 Log 撐爆瀏覽器記憶體）。
- **`src/composables/useCliStream.ts`**：封裝 WebSocket 自動重連、心跳檢測（Ping/Pong）與訊息派發邏輯。
- **`src/components/` (Tailwind CSS 視覺組件)**
- `CommandConsole.vue`：支援 Prompt 模板、工作目錄選擇、超時設定與額外 CLI 旗標開關的指令面板。
- `TerminalViewer.vue`：整合 Xterm.js，直接渲染帶有 ANSI 色彩碼的 CLI 即時輸出。
- `ArtifactInspector.vue`：當 CLI 任務完成並產出結構化報告（如 `result.json` 或程式碼變更 Diff）時，以分頁標籤視覺化呈現結果。

---

## 3. 任務生命週期與資料流設計 (Task Lifecycle & Data Flow)

系統將每一次對 CLI 的呼叫視為一個獨立的「非同步任務（Task）」，其完整資料流如下：

1. **任務派發 (Dispatch)**：
   前端填寫任務Prompt與目標資料夾，發送 `POST /api/tasks`。後端驗證參數合法性後，於 SQLite 建立一筆狀態為 `Queued` 的紀錄，並立即回傳 `task_id` (UUID v4) 給前端。
2. **頻道訂閱 (Subscription)**：
   前端收到 `task_id` 後，透過已建立的 WebSocket 發送訂閱指令 `{ "type": "SUBSCRIBE", "task_id": "..." }`。
3. **排程與沙盒初始化 (Scheduling & Sandbox Init)**：
   後端 `scheduler` 取得 Semaphore 許可後，將狀態更新為 `Running`。系統為該任務建立或指定專屬的工作目錄，並在 Prompt 尾端自動附加系統級規範（例如要求 CLI 完成後將最終摘要寫入 `./.antigravity_out/summary.json`）。
4. **子行程啟動與管道攔截 (Spawn & Pipe Interception)**：
   Rust 透過 `tokio::process::Command::new("antigravity")` 啟動行程，配置 `Stdio::piped()`。同時啟動兩個獨立的 Tokio Task 分別監聽 `stdout` 與 `stderr`。
5. **即時串流推送 (Real-time Streaming)**：
   每當 `BufReader` 讀取到新的一行輸出，立即封裝為 WebSocket 事件 `{ "type": "LOG_CHUNK", "task_id": "...", "stream": "stdout", "data": "..." }` 推送至前端渲染。
6. **任務收斂與結果解析 (Convergence & Artifact Parsing)**：
   當子行程結束並回傳 `ExitStatus` 時，Rust 後端檢查 `./.antigravity_out/summary.json` 是否存在。若存在則讀取並解析該 JSON，將最終狀態與結構化產出一併透過 `{ "type": "TASK_FINISHED", "status": "Completed", "artifact": {...} }` 廣播給前端，同時釋放 Semaphore 許可給下一個排隊任務。

---

## 4. 前後端通訊協定規格 (Communication Protocol Spec)

為發揮 Rust (`serde`) 與 TypeScript 的強型別優勢，WebSocket 通訊全面採用 **Tagged Union (標籤聯合型別)** 格式，確保編譯期與執行期的資料結構一致：

- **後端推送至前端 (Server-to-Client Events)**：
- `TASK_STATE_CHANGED`：通報任務狀態轉換（包含 `task_id`、`old_state`、`new_state`、`timestamp`）。
- `STREAM_OUTPUT`：傳遞即時終端機字串（包含 `task_id`、`channel: "stdout" | "stderr"`、`line: string`、`seq: number`）。
- `ARTIFACT_READY`：當偵測到 CLI 產出指定結果檔時觸發（包含 `task_id`、`file_type: "json" | "markdown" | "diff"`、`payload: any`）。
- `SYSTEM_METRICS`：定時回報目前佇列長度與執行中行程數量。

- **前端發送至後端 (Client-to-Server Commands)**：
- `SUBSCRIBE_TASK` / `UNSUBSCRIBE_TASK`：切換當前監看的任務串流。
- `ABORT_TASK`：使用者於前端點擊「強制停止」，觸發後端對該 `Child` 行程發送終止訊號。

---

## 5. 安全邊界與系統穩定度設計 (Security & Reliability)

1. **根絕命令注入 (Command Injection Prevention)**：
   後端嚴禁呼叫 `sh -c` 或 `cmd /C` 來拼接字串。必須全面使用 Rust 的 `Command::arg()` 與 `Command::args()` 傳遞陣列參數，由作業系統核心直接將參數交給 `antigravity` 執行檔，使任何特殊字元（如 `; rm -rf /` 或反引號）僅被視為純文字 Prompt。
2. **工作目錄白名單與路徑穿越防護 (Path Traversal Guard)**：
   前端指定的工作目錄參數，在後端必須經過 `std::fs::canonicalize` 解析真實路徑，並強制驗證該路徑是否位於預設允許的專案根目錄（Base Workspace）之下，防止 CLI 被誘導去修改系統核心檔案。
3. **非互動模式強制鎖定 (Headless Enforcement)**：
   在 `Command` 初始化時，將 `stdin` 明確設為 `Stdio::null()`，並強制注入 CLI 的非互動/自動授權參數。這樣即使 CLI 意外跳出詢問提示，也會因為讀取到 EOF 而立即報錯退出，不會產生永遠卡住背景執行緒的殭屍行程（Zombie Process）。
4. **背壓與緩衝控制 (Backpressure & Log Throttling)**：
   若 CLI 執行編譯或印出大量 Debug 訊息（每秒數千行），直接逐行透過 WebSocket 發送會導致前端 Vue 渲染執行緒卡死。Rust 後端在讀取 `stdout` 時應設計 **50ms 視窗緩衝（Batching）**，將 50ms 內產生的多行輸出合併為單一陣列封包發送，大幅降低前後端 I/O 負擔。
5. **憑證隔離與存取控制 (Auth & Access Control)**：
   由於此 Web 服務具備直接消耗你個人 Token 與操作檔案系統的能力，Axum 預設應綁定於 `127.0.0.1`（僅限本機或透過 SSH Tunnel / Tailscale 內網存取），並在 HTTP Header 與 WebSocket 握手階段加入 Bearer Token 或 API Key 驗證中介軟體（Middleware）。

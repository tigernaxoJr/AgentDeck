這份規格採用 **Tagged Union（標籤聯合型別）** 設計，在 Rust 端利用 `#[serde(tag = "type", content = "payload")]` 巨集，直接映射至 TypeScript 的 **Discriminated Unions（可辨識聯合）**。這種模式能讓 Vue 3 在處理 WebSocket 訊息時，透過 `switch (msg.type)` 自動獲得完美的型別推導（Type Narrowing），完全不需要手動轉型。

---

## 1. 後端型別定義：`backend-rust/src/models/protocol.rs`

在 `Cargo.toml` 中需啟用 `serde`（含 `derive`）、`uuid`（含 `v4`, `serde`）與 `chrono`（含 `serde`）。

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ==========================================
// 1. 基礎狀態與列舉 (Core Enums)
// ==========================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamChannel {
    Stdout,
    Stderr,
    System, // 用於後端注入的系統提示（如：程序啟動、超時警告）
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Json,
    Markdown,
    Diff,
    PlainText,
}

// ==========================================
// 2. REST API 請求與回應 (HTTP Payloads)
// ==========================================

/// POST /api/tasks 建立任務請求
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub workspace_path: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub extra_flags: Vec<String>,
    /// 是否要求 CLI 強制輸出結構化摘要檔
    #[serde(default = "default_true")]
    pub require_json_summary: bool,
}

fn default_timeout() -> u64 { 600 } // 預設 10 分鐘
fn default_true() -> bool { true }

/// 任務完整資訊（用於 GET /api/tasks 與清單渲染）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMetadata {
    pub task_id: Uuid,
    pub prompt: String,
    pub workspace_path: String,
    pub state: TaskState,
    pub exit_code: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

// ==========================================
// 3. WebSocket 雙向通訊協定 (WS Protocol)
// ==========================================

/// 單行終端機輸出封包（支援 50ms 視窗批次打包）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub seq: u64,
    pub channel: StreamChannel,
    pub line: String,
    pub timestamp: DateTime<Utc>,
}

/// 結構化產出資料封包
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactData {
    pub file_name: String,
    pub artifact_type: ArtifactType,
    /// 若為 JSON 則直接解析為 Value，其餘格式可存放字串於 Value::String
    pub content: Value,
}

/// 【Server -> Client】後端推送至前端的 WebSocket 事件
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerEvent {
    /// 任務狀態發生改變
    #[serde(rename_all = "camelCase")]
    TaskStateChanged {
        task_id: Uuid,
        old_state: Option<TaskState>,
        new_state: TaskState,
        exit_code: Option<i32>,
        timestamp: DateTime<Utc>,
    },

    /// 批次終端機輸出（50ms Throttling Batch）
    #[serde(rename_all = "camelCase")]
    StreamBatch {
        task_id: Uuid,
        logs: Vec<LogEntry>,
    },

    /// 任務完成後解析出的結構化產出
    #[serde(rename_all = "camelCase")]
    ArtifactReady {
        task_id: Uuid,
        artifact: ArtifactData,
    },

    /// 系統佇列與並行負載狀態
    #[serde(rename_all = "camelCase")]
    SystemMetrics {
        running_tasks: usize,
        max_concurrency: usize,
        queued_tasks: usize,
    },

    /// 指令執行錯誤或權限拒絕通知
    #[serde(rename_all = "camelCase")]
    Error {
        task_id: Option<Uuid>,
        code: String,
        message: String,
    },
}

/// 【Client -> Server】前端發送至後端的 WebSocket 指令
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientCommand {
    /// 訂閱特定任務的即時 Log 串流
    #[serde(rename_all = "camelCase")]
    SubscribeTask { task_id: Uuid },

    /// 取消訂閱特定任務串流
    #[serde(rename_all = "camelCase")]
    UnsubscribeTask { task_id: Uuid },

    /// 強制中止執行中的任務 (發送 SIGTERM/SIGKILL)
    #[serde(rename_all = "camelCase")]
    AbortTask { task_id: Uuid },

    /// 前端心跳檢測
    Ping,
}

```

---

## 2. 前端型別定義：`frontend-vue/src/types/protocol.ts`

此檔案與上述 Rust 結構 **100% 嚴格對齊**（注意 Rust 端的 `#[serde(rename_all = "camelCase")]` 會將底線命名自動轉為駝峰命名，符合前端開發慣例）。

```typescript
// ==========================================
// 1. 基礎狀態與列舉 (Core Types)
// ==========================================

export type TaskState =
  | "QUEUED"
  | "RUNNING"
  | "COMPLETED"
  | "FAILED"
  | "CANCELLED"
  | "TIMEOUT";

export type StreamChannel = "stdout" | "stderr" | "system";

export type ArtifactType = "json" | "markdown" | "diff" | "plaintext";

// ==========================================
// 2. REST API 請求與回應介面 (HTTP Interfaces)
// ==========================================

export interface CreateTaskRequest {
  prompt: string;
  workspacePath: string;
  timeoutSecs?: number;
  extraFlags?: string[];
  requireJsonSummary?: boolean;
}

export interface TaskMetadata {
  taskId: string;
  prompt: string;
  workspacePath: string;
  state: TaskState;
  exitCode: number | null;
  createdAt: string; // ISO 8601 UTC String
  startedAt: string | null;
  finishedAt: string | null;
}

// ==========================================
// 3. WebSocket 雙向通訊協定 (Discriminated Unions)
// ==========================================

export interface LogEntry {
  seq: number;
  channel: StreamChannel;
  line: string;
  timestamp: string;
}

export interface ArtifactData<T = unknown> {
  fileName: string;
  artifactType: ArtifactType;
  content: T;
}

/**
 * 【Server -> Client】後端推送事件各別 Payload 定義
 */
export interface TaskStateChangedPayload {
  taskId: string;
  oldState: TaskState | null;
  newState: TaskState;
  exitCode: number | null;
  timestamp: string;
}

export interface StreamBatchPayload {
  taskId: string;
  logs: LogEntry[];
}

export interface ArtifactReadyPayload {
  taskId: string;
  artifact: ArtifactData;
}

export interface SystemMetricsPayload {
  runningTasks: number;
  maxConcurrency: number;
  queuedTasks: number;
}

export interface ErrorPayload {
  taskId: string | null;
  code: string;
  message: string;
}

/**
 * 【Server -> Client】聯合事件型別 (支援 TypeScript 自動型別收窄)
 */
export type ServerEvent =
  | { type: "TASK_STATE_CHANGED"; payload: TaskStateChangedPayload }
  | { type: "STREAM_BATCH"; payload: StreamBatchPayload }
  | { type: "ARTIFACT_READY"; payload: ArtifactReadyPayload }
  | { type: "SYSTEM_METRICS"; payload: SystemMetricsPayload }
  | { type: "ERROR"; payload: ErrorPayload };

/**
 * 【Client -> Server】前端發送指令型別
 */
export type ClientCommand =
  | { type: "SUBSCRIBE_TASK"; payload: { taskId: string } }
  | { type: "UNSUBSCRIBE_TASK"; payload: { taskId: string } }
  | { type: "ABORT_TASK"; payload: { taskId: string } }
  | { type: "PING" };
```

---

## 3. 序列化對接範例 (JSON Wire Format)

當 Rust 後端執行 `serde_json::to_string(&event)` 推送至 WebSocket 時，實際傳輸的 JSON 結構如下：

```json
{
  "type": "STREAM_BATCH",
  "payload": {
    "taskId": "01928a7b-6c4d-7e8f-9a0b-1c2d3e4f5a6b",
    "logs": [
      {
        "seq": 42,
        "channel": "stdout",
        "line": "\u001b[32m✓\u001b[0m Analyzing workspace dependencies...",
        "timestamp": "2026-10-14T13:48:05.120Z"
      },
      {
        "seq": 43,
        "channel": "stdout",
        "line": "Generating refactored module structure...",
        "timestamp": "2026-10-14T13:48:05.145Z"
      }
    ]
  }
}
```

### 設計巧思與實作建議：

- **序號控制 (`seq: u64`)**：每個 `LogEntry` 都帶有嚴格遞增的 `seq`。如果前端因為切換分頁或短暫斷線重連，可以利用最後收到的 `seq` 向後端請求補齊遺漏的區段，或者確保批次陣列在 Xterm.js 寫入時不會亂序。
- **未來擴充免手動同步 (`ts-rs`)**：如果專案規模擴大，可以在 Rust 端引入 `ts-rs` crate，直接在 Rust Struct 上標註 `#[derive(TS)]` 與 `#[ts(export)]`，每次執行 `cargo test` 時就會自動編譯並覆寫前端的 `frontend-vue/src/types/protocol.ts`，達成零誤差的 Single Source of Truth。

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
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub extra_flags: Vec<String>,
    /// 是否要求 CLI 強制輸出結構化摘要檔
    #[serde(default = "default_true")]
    pub require_json_summary: bool,
}

fn default_timeout() -> u64 {
    600
} // 預設 10 分鐘
fn default_true() -> bool {
    true
}

/// Token 使用量統計
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(alias = "input_tokens", rename(serialize = "inputTokens"))]
    pub input_tokens: i64,
    #[serde(alias = "output_tokens", rename(serialize = "outputTokens"))]
    pub output_tokens: i64,
    #[serde(alias = "thinking_tokens", rename(serialize = "thinkingTokens"))]
    pub thinking_tokens: i64,
    #[serde(alias = "cache_read_tokens", rename(serialize = "cacheReadTokens"))]
    pub cache_read_tokens: i64,
    #[serde(alias = "total_tokens", rename(serialize = "totalTokens"))]
    pub total_tokens: i64,
}

/// 任務完整資訊（用於 GET /api/tasks 與清單渲染）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMetadata {
    pub task_id: Uuid,
    pub prompt: String,
    pub workspace_path: String,
    #[serde(default)]
    pub model: Option<String>,
    pub state: TaskState,
    pub exit_code: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub usage: Option<TokenUsage>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
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

// ==========================================
// 4. Git 相關資料結構 (Git Diff & Ops)
// ==========================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFileStat {
    /// 狀態碼："M" (修改), "A" (新增/未追蹤), "D" (刪除), "R" (重新命名)
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffReport {
    pub is_git_repo: bool,
    pub branch: String,
    pub changed_files: Vec<ChangedFileStat>,
    pub unified_diff: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertFileRequest {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitChangesRequest {
    pub message: Option<String>,
}

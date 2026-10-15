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

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  thinkingTokens: number;
  cacheReadTokens: number;
  totalTokens: number;
}

export interface ModelInfo {
  id: string;
  name: string;
}

export interface WorkspaceInfo {
  path: string;
  name: string;
}

// ==========================================
// 2. REST API 請求與回應介面 (HTTP Interfaces)
// ==========================================

export interface CreateTaskRequest {
  prompt: string;
  workspacePath: string;
  model?: string;
  timeoutSecs?: number;
  extraFlags?: string[];
  requireJsonSummary?: boolean;
}

export interface TaskMetadata {
  taskId: string;
  prompt: string;
  workspacePath: string;
  model?: string;
  state: TaskState;
  exitCode: number | null;
  createdAt: string; // ISO 8601 UTC String
  startedAt: string | null;
  finishedAt: string | null;
  usage?: TokenUsage | null;
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
  usage?: TokenUsage | null;
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

// ==========================================
// 4. Git 相關型別
// ==========================================

export interface ChangedFileStat {
  status: "M" | "A" | "D" | "R" | string;
  path: string;
}

export interface GitDiffReport {
  isGitRepo: boolean;
  branch: string;
  changedFiles: ChangedFileStat[];
  unifiedDiff: string;
}

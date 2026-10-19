import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { useWebSocket, useStorage } from "@vueuse/core";
import type {
  TaskMetadata,
  CreateTaskRequest,
  ServerEvent,
  ClientCommand,
  LogEntry,
  ArtifactData,
  SystemMetricsPayload,
  GitDiffReport,
  ModelInfo,
  WorkspaceInfo,
} from "@/types/protocol";

// 預設走同源（dev 由 Vite proxy、Docker 由 nginx 轉發），不需跨域；
// 需要直連後端時才設定 VITE_API_BASE（並把前端來源加入後端 CORS_ORIGINS）。
const API_BASE: string = import.meta.env.VITE_API_BASE || "";
const MAX_LOG_BUFFER = 5000; // 每個任務在記憶體中最多保留 5000 行，防止瀏覽器 OOM

export const useTaskStore = defineStore("taskStore", () => {
  // ==========================================
  // 1. 核心狀態 (State)
  // ==========================================
  const apiKey = useStorage<string>("antigravity_api_key", "");
  const isAuthError = ref<boolean>(false);

  const tasks = ref<Map<string, TaskMetadata>>(new Map());
  const logsByTask = ref<Map<string, LogEntry[]>>(new Map());
  const artifactsByTask = ref<Map<string, ArtifactData>>(new Map());
  const diffsByTask = ref<Map<string, GitDiffReport>>(new Map());
  const isDiffLoading = ref<boolean>(false);

  const availableModels = ref<ModelInfo[]>([]);
  const isModelsLoading = ref<boolean>(false);

  const availableWorkspaces = ref<WorkspaceInfo[]>([]);
  const isWorkspacesLoading = ref<boolean>(false);

  const activeTaskId = ref<string | null>(null);
  const lastError = ref<string | null>(null);

  const metrics = ref<SystemMetricsPayload>({
    runningTasks: 0,
    maxConcurrency: 2,
    queuedTasks: 0,
  });

  // ==========================================
  // 2. 衍生狀態 (Getters)
  // ==========================================
  const taskList = computed(() =>
    Array.from(tasks.value.values()).sort(
      (a, b) =>
        new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
    ),
  );

  const activeTask = computed(() =>
    activeTaskId.value ? (tasks.value.get(activeTaskId.value) ?? null) : null,
  );

  const activeLogs = computed(() =>
    activeTaskId.value ? (logsByTask.value.get(activeTaskId.value) ?? []) : [],
  );

  const activeArtifact = computed(() =>
    activeTaskId.value
      ? (artifactsByTask.value.get(activeTaskId.value) ?? null)
      : null,
  );

  const activeDiff = computed(() =>
    activeTaskId.value
      ? (diffsByTask.value.get(activeTaskId.value) ?? null)
      : null,
  );

  // ==========================================
  // 3. WebSocket 連線與事件分發
  // ==========================================
  const wsUrl = computed(() => {
    const origin = API_BASE || window.location.origin;
    const baseWs = origin.replace(/^http/, "ws") + "/api/ws";
    const tokenParam = encodeURIComponent(apiKey.value.trim());
    return tokenParam ? `${baseWs}?token=${tokenParam}` : baseWs;
  });

  const {
    status: wsStatus,
    send: wsSend,
  } = useWebSocket(wsUrl, {
    autoReconnect: {
      retries: 10,
      delay: 2000,
    },
    heartbeat: {
      message: JSON.stringify({ type: "PING" } satisfies ClientCommand),
      interval: 15000,
    } as any,
    onConnected() {
      isAuthError.value = false;
      // 斷線重連成功後，若當前有選中的任務，自動重新訂閱其即時串流
      if (activeTaskId.value) {
        sendCommand({
          type: "SUBSCRIBE_TASK",
          payload: { taskId: activeTaskId.value },
        });
      }
    },
    onMessage(_ws, event) {
      try {
        const msg = JSON.parse(event.data) as ServerEvent;
        handleServerEvent(msg);
      } catch (err) {
        console.error("無法解析 WebSocket 訊息:", err);
      }
    },
  });

  function sendCommand(cmd: ClientCommand) {
    wsSend(JSON.stringify(cmd));
  }

  function handleServerEvent(event: ServerEvent) {
    switch (event.type) {
      case "TASK_STATE_CHANGED": {
        const { taskId, newState, exitCode, timestamp, usage } = event.payload;
        const existing = tasks.value.get(taskId);
        if (existing) {
          existing.state = newState;
          if (exitCode !== null) existing.exitCode = exitCode;
          if (newState === "RUNNING") existing.startedAt = timestamp;
          if (
            ["COMPLETED", "FAILED", "CANCELLED", "TIMEOUT"].includes(newState)
          ) {
            existing.finishedAt = timestamp;
          }
          if (usage) {
            existing.usage = usage;
          }
          tasks.value.set(taskId, { ...existing });
        } else {
          fetchTasks();
        }
        break;
      }

      case "STREAM_BATCH": {
        const { taskId, logs } = event.payload;
        const currentBuffer = logsByTask.value.get(taskId) ?? [];
        currentBuffer.push(...logs);

        if (currentBuffer.length > MAX_LOG_BUFFER) {
          currentBuffer.splice(0, currentBuffer.length - MAX_LOG_BUFFER);
        }
        logsByTask.value.set(taskId, currentBuffer);
        break;
      }

      case "ARTIFACT_READY": {
        const { taskId, artifact } = event.payload;
        if (artifact.artifactType === "diff") {
          try {
            diffsByTask.value.set(
              taskId,
              typeof artifact.content === "string"
                ? JSON.parse(artifact.content)
                : (artifact.content as GitDiffReport),
            );
          } catch {
            // fallback
          }
        } else {
          artifactsByTask.value.set(taskId, artifact);
        }
        break;
      }

      case "SYSTEM_METRICS": {
        metrics.value = event.payload;
        break;
      }

      case "ERROR": {
        lastError.value = `[${event.payload.code}] ${event.payload.message}`;
        break;
      }
    }
  }

  // ==========================================
  // 4. REST API 封裝 (含 Bearer Token)
  // ==========================================
  async function authFetch(
    path: string,
    options: RequestInit = {},
  ): Promise<Response> {
    const headers = new Headers(options.headers);
    if (apiKey.value.trim()) {
      headers.set("Authorization", `Bearer ${apiKey.value.trim()}`);
    }
    if (options.body && !headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }

    const res = await fetch(`${API_BASE}${path}`, {
      ...options,
      headers,
    });

    if (res.status === 401) {
      isAuthError.value = true;
      lastError.value =
        "[401 UNAUTHORIZED] API Key 驗證失敗，請設定正確的存取金鑰。";
      throw new Error("Unauthorized");
    }

    return res;
  }

  /** 載入所有歷史任務清單 */
  async function fetchTasks() {
    try {
      const res = await authFetch("/api/tasks");
      if (!res.ok) throw new Error("無法載入任務清單");
      const list = (await res.json()) as TaskMetadata[];

      const newMap = new Map<string, TaskMetadata>();
      for (const item of list) {
        newMap.set(item.taskId, item);
      }
      tasks.value = newMap;

      if (!activeTaskId.value && list.length > 0) {
        selectTask(list[0].taskId);
      }
    } catch (err: any) {
      console.warn("fetchTasks error:", err.message);
    }
  }

  /** 建立並派發新任務 */
  async function createTask(req: CreateTaskRequest): Promise<TaskMetadata> {
    lastError.value = null;
    const res = await authFetch("/api/tasks", {
      method: "POST",
      body: JSON.stringify(req),
    });

    if (!res.ok) {
      const errText = await res.text();
      lastError.value = errText;
      throw new Error(errText);
    }

    const created = (await res.json()) as TaskMetadata;
    tasks.value.set(created.taskId, created);
    logsByTask.value.set(created.taskId, []);

    selectTask(created.taskId);
    return created;
  }

  /** 切換當前監看的任務 */
  function selectTask(taskId: string) {
    if (activeTaskId.value === taskId) return;

    if (activeTaskId.value) {
      sendCommand({
        type: "UNSUBSCRIBE_TASK",
        payload: { taskId: activeTaskId.value },
      });
    }

    activeTaskId.value = taskId;
    sendCommand({
      type: "SUBSCRIBE_TASK",
      payload: { taskId },
    });
  }

  /** 強制中止執行中或排隊中的任務 */
  function abortTask(taskId: string) {
    sendCommand({
      type: "ABORT_TASK",
      payload: { taskId },
    });
  }

  /** 取得工作目錄的 Git Diff */
  async function fetchTaskDiff(taskId: string) {
    isDiffLoading.value = true;
    try {
      const res = await authFetch(`/api/tasks/${taskId}/diff`);
      if (res.ok) {
        const report = (await res.json()) as GitDiffReport;
        diffsByTask.value.set(taskId, report);
      }
    } catch (err) {
      console.error("fetchTaskDiff error:", err);
    } finally {
      isDiffLoading.value = false;
    }
  }

  /** 還原單一檔案變更 */
  async function revertFile(taskId: string, filePath: string) {
    const res = await authFetch(`/api/tasks/${taskId}/diff/revert`, {
      method: "POST",
      body: JSON.stringify({ filePath }),
    });
    if (!res.ok) throw new Error(await res.text());
    await fetchTaskDiff(taskId);
  }

  /** 一鍵安全 Commit */
  async function commitChanges(taskId: string, message?: string) {
    const res = await authFetch(`/api/tasks/${taskId}/diff/commit`, {
      method: "POST",
      body: JSON.stringify({ message }),
    });
    if (!res.ok) throw new Error(await res.text());
    await fetchTaskDiff(taskId);
  }

  /** 查詢 CLI 支援的模型清單 */
  async function fetchModels() {
    isModelsLoading.value = true;
    try {
      const res = await authFetch("/api/models");
      if (res.ok) {
        const list = (await res.json()) as ModelInfo[];
        availableModels.value = list;
      }
    } catch (err) {
      console.warn("fetchModels error:", err);
    } finally {
      isModelsLoading.value = false;
    }
  }

  /** 查詢可用工作目錄清單 */
  async function fetchWorkspaces() {
    isWorkspacesLoading.value = true;
    try {
      const res = await authFetch("/api/workspaces");
      if (res.ok) {
        const list = (await res.json()) as WorkspaceInfo[];
        availableWorkspaces.value = list;
      }
    } catch (err) {
      console.warn("fetchWorkspaces error:", err);
    } finally {
      isWorkspacesLoading.value = false;
    }
  }

  return {
    apiKey,
    isAuthError,
    wsStatus,
    tasks,
    taskList,
    activeTaskId,
    activeTask,
    activeLogs,
    activeArtifact,
    activeDiff,
    isDiffLoading,
    availableModels,
    isModelsLoading,
    availableWorkspaces,
    isWorkspacesLoading,
    metrics,
    lastError,
    fetchTasks,
    fetchModels,
    fetchWorkspaces,
    createTask,
    selectTask,
    abortTask,
    fetchTaskDiff,
    revertFile,
    commitChanges,
  };
});

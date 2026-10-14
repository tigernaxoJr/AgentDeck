這份前端實作採用 **Vue 3 (`<script setup lang="ts">`) + Pinia + Tailwind CSS**，並整合 **Xterm.js** 來渲染 Antigravity CLI 輸出的原生 ANSI 色彩碼。

為了確保當 CLI 瞬間傾倒數萬行 Log 時不會塞爆瀏覽器記憶體或造成 Vue 響應式追蹤（Reactivity Tracking）卡死，我們在 Pinia Store 中採用了 **Ring Buffer（環狀緩衝區上限控制）**，並在組件層透過 **增量游標（Incremental Cursor）** 將新收到的批次日誌直接寫入 Xterm.js 的 Canvas/WebGL 渲染層。

在前端專案中需安裝以下套件：

```bash
npm install pinia @vueuse/core @xterm/xterm @xterm/addon-fit

```

---

## 1. 狀態與 WebSocket 串流管理：`frontend-vue/src/stores/useTaskStore.ts`

此 Store 整合了 `@vueuse/core` 的 `useWebSocket`（支援斷線自動重連與心跳），統一管理 REST API 請求、WebSocket 頻道訂閱切換，以及結構化產出（Artifact）的接收。

```typescript
import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { useWebSocket } from "@vueuse/core";
import type {
  TaskMetadata,
  CreateTaskRequest,
  ServerEvent,
  ClientCommand,
  LogEntry,
  ArtifactData,
  SystemMetricsPayload,
} from "@/types/protocol";

const API_BASE = import.meta.env.VITE_API_BASE || "http://127.0.0.1:3000";
const WS_URL = API_BASE.replace(/^http/, "ws") + "/api/ws";
const MAX_LOG_BUFFER = 5000; // 每個任務在記憶體中最多保留 5000 行，防止瀏覽器 OOM

export const useTaskStore = defineStore("taskStore", () => {
  // ==========================================
  // 1. 核心狀態 (State)
  // ==========================================
  const tasks = ref<Map<string, TaskMetadata>>(new Map());
  const logsByTask = ref<Map<string, LogEntry[]>>(new Map());
  const artifactsByTask = ref<Map<string, ArtifactData>>(new Map());
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

  // ==========================================
  // 3. WebSocket 連線與事件分發 (WebSocket & Dispatcher)
  // ==========================================
  const { status: wsStatus, send: wsSend } = useWebSocket(WS_URL, {
    autoReconnect: {
      retries: 10,
      delay: 2000,
    },
    heartbeat: {
      message: JSON.stringify({ type: "PING" } satisfies ClientCommand),
      interval: 15000,
    },
    onConnected() {
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

  /**
   * 依靠 TypeScript Discriminated Unions 自動推導各分支的 payload 型別
   */
  function handleServerEvent(event: ServerEvent) {
    switch (event.type) {
      case "TASK_STATE_CHANGED": {
        const { taskId, newState, exitCode, timestamp } = event.payload;
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
          // 觸發 Map 更新
          tasks.value.set(taskId, { ...existing });
        } else {
          // 若為其他視窗建立的新任務，重新拉取清單
          fetchTasks();
        }
        break;
      }

      case "STREAM_BATCH": {
        const { taskId, logs } = event.payload;
        const currentBuffer = logsByTask.value.get(taskId) ?? [];
        currentBuffer.push(...logs);

        // Ring Buffer 裁切：超過上限時捨棄最舊的 Log
        if (currentBuffer.length > MAX_LOG_BUFFER) {
          currentBuffer.splice(0, currentBuffer.length - MAX_LOG_BUFFER);
        }
        logsByTask.value.set(taskId, currentBuffer);
        break;
      }

      case "ARTIFACT_READY": {
        const { taskId, artifact } = event.payload;
        artifactsByTask.value.set(taskId, artifact);
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
  // 4. 動作與 API 呼叫 (Actions)
  // ==========================================

  /** 載入所有歷史任務清單 */
  async function fetchTasks() {
    const res = await fetch(`${API_BASE}/api/tasks`);
    if (!res.ok) throw new Error("無法載入任務清單");
    const list = (await res.json()) as TaskMetadata[];

    const newMap = new Map<string, TaskMetadata>();
    for (const item of list) {
      newMap.set(item.taskId, item);
    }
    tasks.value = newMap;

    // 若尚未選中任何任務且清單不為空，預設選中最新的一筆
    if (!activeTaskId.value && list.length > 0) {
      selectTask(list[0].taskId);
    }
  }

  /** 建立並派發新 CLI 任務 */
  async function createTask(req: CreateTaskRequest): Promise<TaskMetadata> {
    lastError.value = null;
    const res = await fetch(`${API_BASE}/api/tasks`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
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

    // 自動切換並訂閱新建立的任務
    selectTask(created.taskId);
    return created;
  }

  /** 切換當前監看的任務（自動退訂舊任務、訂閱新任務） */
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

  return {
    wsStatus,
    tasks,
    taskList,
    activeTaskId,
    activeTask,
    activeLogs,
    activeArtifact,
    metrics,
    lastError,
    fetchTasks,
    createTask,
    selectTask,
    abortTask,
  };
});
```

---

## 2. 終端機與任務控制台組件：`frontend-vue/src/components/AntigravityConsole.vue`

此組件結合了 **指令發送面板**、**左側任務排程佇列**、**Xterm.js 即時終端機** 與 **JSON Artifact 結構化檢視器**。透過 `ResizeObserver` 與 `@xterm/addon-fit`，終端機畫布能自動適應視窗縮放。

```vue
<template>
  <div
    class="flex h-screen w-screen bg-slate-950 text-slate-100 font-sans overflow-hidden"
  >
    <!-- 左側欄：系統指標與任務佇列清單 -->
    <aside class="w-80 border-r border-slate-800 bg-slate-900/60 flex flex-col">
      <!-- 頂部標題與連線狀態 -->
      <div
        class="p-4 border-b border-slate-800 flex items-center justify-between"
      >
        <div class="flex items-center gap-2">
          <span class="h-2.5 w-2.5 rounded-full" :class="wsStatusColor"></span>
          <h1 class="font-bold text-sm tracking-wide text-slate-200">
            ANTIGRAVITY CONSOLE
          </h1>
        </div>
        <span
          class="text-xs font-mono px-2 py-0.5 rounded bg-slate-800 text-slate-400"
        >
          {{ store.metrics.runningTasks }}/{{
            store.metrics.maxConcurrency
          }}
          執行中
        </span>
      </div>

      <!-- 佇列負載提示列 -->
      <div
        class="px-4 py-2 bg-slate-900 border-b border-slate-800/80 flex justify-between text-xs text-slate-400"
      >
        <span>排隊等待中 (Queued)</span>
        <span class="font-mono font-semibold text-amber-400">{{
          store.metrics.queuedTasks
        }}</span>
      </div>

      <!-- 任務歷史與排程清單 -->
      <div class="flex-1 overflow-y-auto divide-y divide-slate-800/60">
        <button
          v-for="task in store.taskList"
          :key="task.taskId"
          @click="store.selectTask(task.taskId)"
          class="w-full text-left p-3.5 transition-colors hover:bg-slate-800/50 flex flex-col gap-1.5"
          :class="{
            'bg-slate-800/90 border-l-2 border-indigo-500':
              store.activeTaskId === task.taskId,
          }"
        >
          <div class="flex items-center justify-between">
            <span
              class="text-[11px] font-mono font-semibold px-1.5 py-0.5 rounded"
              :class="stateBadgeClass(task.state)"
            >
              {{ task.state }}
            </span>
            <span class="text-[11px] font-mono text-slate-500">
              {{ formatTime(task.createdAt) }}
            </span>
          </div>
          <p class="text-xs text-slate-200 line-clamp-2 font-medium break-all">
            {{ task.prompt }}
          </p>
          <span class="text-[11px] font-mono text-slate-500 truncate">
            📁 {{ task.workspacePath }}
          </span>
        </button>

        <div
          v-if="store.taskList.length === 0"
          class="p-8 text-center text-xs text-slate-500"
        >
          尚無任何執行任務
        </div>
      </div>
    </aside>

    <!-- 右側主工作區：指令輸入 + 終端機 / 結構化結果檢視器 -->
    <main class="flex-1 flex flex-col min-w-0">
      <!-- 頂部：指令派發表單 -->
      <section
        class="p-4 bg-slate-900/40 border-b border-slate-800 flex flex-col gap-3"
      >
        <div class="flex gap-3">
          <div class="w-64">
            <label class="block text-[11px] font-mono text-slate-400 mb-1"
              >目標工作目錄 (Workspace)</label
            >
            <input
              v-model="form.workspacePath"
              type="text"
              placeholder="例如: ./my-app"
              class="w-full bg-slate-950 border border-slate-700 rounded px-3 py-1.5 text-xs font-mono text-slate-200 focus:outline-none focus:border-indigo-500"
            />
          </div>
          <div class="w-32">
            <label class="block text-[11px] font-mono text-slate-400 mb-1"
              >超時限制 (秒)</label
            >
            <input
              v-model.number="form.timeoutSecs"
              type="number"
              min="30"
              max="3600"
              class="w-full bg-slate-950 border border-slate-700 rounded px-3 py-1.5 text-xs font-mono text-slate-200 focus:outline-none focus:border-indigo-500"
            />
          </div>
          <div class="flex items-end pb-1">
            <label
              class="flex items-center gap-2 text-xs text-slate-300 cursor-pointer select-none"
            >
              <input
                v-model="form.requireJsonSummary"
                type="checkbox"
                class="rounded bg-slate-950 border-slate-700 text-indigo-600 focus:ring-0"
              />
              <span>要求產出結構化 JSON 摘要</span>
            </label>
          </div>
        </div>

        <div class="flex gap-3">
          <textarea
            v-model="form.prompt"
            rows="2"
            placeholder="輸入要交給 Antigravity CLI 執行的任務指令 (Ctrl + Enter 發送)..."
            @keydown.ctrl.enter.prevent="submitTask"
            class="flex-1 bg-slate-950 border border-slate-700 rounded px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:outline-none focus:border-indigo-500 resize-none"
          ></textarea>
          <button
            @click="submitTask"
            :disabled="isSubmitting || !form.prompt.trim()"
            class="px-5 bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-800 disabled:text-slate-500 text-white font-semibold text-xs rounded transition-colors flex items-center gap-2 shrink-0 cursor-pointer"
          >
            <span>🚀 派發任務</span>
          </button>
        </div>

        <!-- 錯誤提示橫幅 -->
        <div
          v-if="store.lastError"
          class="px-3 py-2 rounded bg-rose-950/80 border border-rose-800 text-rose-200 text-xs font-mono flex justify-between items-center"
        >
          <span>{{ store.lastError }}</span>
          <button
            @click="store.lastError = null"
            class="text-rose-400 hover:text-white"
          >
            ✕
          </button>
        </div>
      </section>

      <!-- 中間控制列：分頁切換（即時終端機 vs 結構化產出）與中止按鈕 -->
      <header
        class="px-4 py-2 bg-slate-900 border-b border-slate-800 flex items-center justify-between"
      >
        <div class="flex items-center gap-2">
          <button
            @click="activeTab = 'terminal'"
            class="px-3 py-1 text-xs font-medium rounded transition-colors"
            :class="
              activeTab === 'terminal'
                ? 'bg-slate-800 text-white'
                : 'text-slate-400 hover:text-slate-200'
            "
          >
            🖥️ 即時終端機 (stdout / stderr)
          </button>
          <button
            @click="activeTab = 'artifact'"
            class="px-3 py-1 text-xs font-medium rounded transition-colors flex items-center gap-1.5"
            :class="
              activeTab === 'artifact'
                ? 'bg-slate-800 text-white'
                : 'text-slate-400 hover:text-slate-200'
            "
          >
            <span>📦 結構化產出 (Artifact)</span>
            <span
              v-if="store.activeArtifact"
              class="h-2 w-2 rounded-full bg-emerald-400 animate-pulse"
            ></span>
          </button>
        </div>

        <!-- 當前任務操作與強制中止 -->
        <div v-if="store.activeTask" class="flex items-center gap-3">
          <span class="text-xs font-mono text-slate-400">
            Task ID: {{ store.activeTask.taskId.slice(0, 8) }}
          </span>
          <button
            v-if="
              store.activeTask.state === 'RUNNING' ||
              store.activeTask.state === 'QUEUED'
            "
            @click="store.abortTask(store.activeTask.taskId)"
            class="px-3 py-1 bg-rose-600/20 hover:bg-rose-600 border border-rose-500/40 text-rose-300 hover:text-white text-xs font-semibold rounded transition-colors cursor-pointer"
          >
            ⏹ 強制中止 (SIGTERM)
          </button>
        </div>
      </header>

      <!-- 底部顯示區：Xterm.js 畫布 或 JSON Artifact 檢視器 -->
      <div class="flex-1 relative bg-[#090d16] overflow-hidden">
        <!-- 分頁 1：Xterm.js 終端機容器 -->
        <div
          v-show="activeTab === 'terminal'"
          ref="terminalContainer"
          class="w-full h-full p-2"
        ></div>

        <!-- 分頁 2：結構化 JSON / Markdown 產出檢視器 -->
        <div
          v-if="activeTab === 'artifact'"
          class="w-full h-full overflow-y-auto p-6 bg-slate-950"
        >
          <div v-if="store.activeArtifact" class="max-w-4xl mx-auto space-y-4">
            <div
              class="flex items-center justify-between border-b border-slate-800 pb-3"
            >
              <div>
                <h2 class="text-sm font-bold text-slate-200">
                  {{ store.activeArtifact.fileName }}
                </h2>
                <p class="text-xs text-slate-400 font-mono mt-0.5">
                  Type: {{ store.activeArtifact.artifactType.toUpperCase() }}
                </p>
              </div>
            </div>
            <pre
              class="p-4 rounded-lg bg-slate-900 border border-slate-800 text-xs font-mono text-emerald-300 overflow-x-auto leading-relaxed"
              >{{ formatArtifact(store.activeArtifact.content) }}</pre
            >
          </div>
          <div
            v-else
            class="h-full flex items-center justify-center text-xs text-slate-500 font-mono"
          >
            此任務尚無結構化產出，或任務仍在執行中...
          </div>
        </div>
      </div>
    </main>
  </div>
</template>

<script setup lang="ts">
import {
  ref,
  reactive,
  computed,
  watch,
  onMounted,
  onBeforeUnmount,
} from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { useTaskStore } from "@/stores/useTaskStore";
import type { LogEntry, TaskState } from "@/types/protocol";

const store = useTaskStore();

// 表單狀態
const isSubmitting = ref(false);
const activeTab = ref<"terminal" | "artifact">("terminal");
const form = reactive({
  prompt: "",
  workspacePath: ".",
  timeoutSecs: 600,
  requireJsonSummary: true,
});

// Xterm.js 實例與增量渲染游標
const terminalContainer = ref<HTMLElement | null>(null);
let term: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let resizeObserver: ResizeObserver | null = null;
let renderedCount = 0; // 記錄目前已寫入 Xterm 的 Log 陣列索引，實現 O(1) 增量附加

// ==========================================
// 1. 初始化 Xterm.js 與視窗自適應
// ==========================================
onMounted(async () => {
  initTerminal();
  await store.fetchTasks();
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  term?.dispose();
});

function initTerminal() {
  if (!terminalContainer.value) return;

  term = new Terminal({
    cursorBlink: false,
    disableStdin: true, // 純唯讀輸出面板，禁止前端鍵盤輸入
    convertEol: true,
    fontFamily: '"JetBrains Mono", "Fira Code", monospace',
    fontSize: 13,
    lineHeight: 1.35,
    theme: {
      background: "#090d16",
      foreground: "#e2e8f0",
      cursor: "#6366f1",
      selectionBackground: "#334155",
    },
  });

  fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(terminalContainer.value);
  fitAddon.fit();

  // 監聽容器尺寸變化並自動調整 Xterm 行列數
  resizeObserver = new ResizeObserver(() => {
    if (activeTab.value === "terminal") {
      fitAddon?.fit();
    }
  });
  resizeObserver.observe(terminalContainer.value);
}

// ==========================================
// 2. 將單筆 Log 依頻道著色寫入 Xterm.js
// ==========================================
function writeLogToXterm(entry: LogEntry) {
  if (!term) return;

  if (entry.channel === "system") {
    // 系統事件以青藍色高亮顯示
    term.writeln(`\x1b[36m[SYSTEM]\x1b[0m ${entry.line}`);
  } else if (entry.channel === "stderr") {
    // 若 CLI 本身沒帶 ANSI 色碼，將 stderr 預設染成淡紅色方便除錯
    term.writeln(`\x1b[31m${entry.line}\x1b[0m`);
  } else {
    // stdout 原生保留 Antigravity CLI 自己的 ANSI 色碼
    term.writeln(entry.line);
  }
}

// ==========================================
// 3. 監聽任務切換與 50ms 批次串流增量寫入
// ==========================================

// 當切換選中的任務時，清空畫面並重繪該任務已緩存的所有 Log
watch(
  () => store.activeTaskId,
  () => {
    if (!term) return;
    term.clear();
    renderedCount = 0;

    const logs = store.activeLogs;
    for (const entry of logs) {
      writeLogToXterm(entry);
    }
    renderedCount = logs.length;
  },
);

// 當當前任務收到新的 STREAM_BATCH 時，僅將新增的區段寫入 Xterm
watch(
  () => store.activeLogs.length,
  (newLen) => {
    if (!term) return;
    const logs = store.activeLogs;

    // 若觸發了 Ring Buffer 裁切導致長度小於游標，則全量重繪
    if (newLen < renderedCount) {
      term.clear();
      for (const entry of logs) {
        writeLogToXterm(entry);
      }
      renderedCount = newLen;
      return;
    }

    // 僅針對新推入的批次執行增量寫入
    for (let i = renderedCount; i < newLen; i++) {
      writeLogToXterm(logs[i]);
    }
    renderedCount = newLen;
  },
);

// 當收到結構化產出時，若任務已完成可自動提示或切換檢視
watch(
  () => store.activeArtifact,
  (artifact) => {
    if (artifact && term) {
      term.writeln(
        `\x1b[32m[ARTIFACT READY]\x1b[0m 已捕獲結構化結果檔 (${artifact.fileName})，請點擊上方「結構化產出」分頁檢視。`,
      );
    }
  },
);

// ==========================================
// 4. 輔助函式與 UI 狀態映射
// ==========================================
async function submitTask() {
  if (!form.prompt.trim() || isSubmitting.value) return;
  isSubmitting.value = true;
  try {
    await store.createTask({
      prompt: form.prompt,
      workspacePath: form.workspacePath,
      timeoutSecs: form.timeoutSecs,
      requireJsonSummary: form.requireJsonSummary,
    });
    form.prompt = "";
    activeTab.value = "terminal";
  } finally {
    isSubmitting.value = false;
  }
}

const wsStatusColor = computed(() => {
  switch (store.wsStatus) {
    case "OPEN":
      return "bg-emerald-400 shadow-[0_0_8px_#34d399]";
    case "CONNECTING":
      return "bg-amber-400 animate-ping";
    default:
      return "bg-rose-500";
  }
});

function stateBadgeClass(state: TaskState): string {
  switch (state) {
    case "RUNNING":
      return "bg-indigo-500/20 text-indigo-300 border border-indigo-500/40 animate-pulse";
    case "QUEUED":
      return "bg-amber-500/20 text-amber-300 border border-amber-500/40";
    case "COMPLETED":
      return "bg-emerald-500/20 text-emerald-300 border border-emerald-500/40";
    case "FAILED":
    case "TIMEOUT":
      return "bg-rose-500/20 text-rose-300 border border-rose-500/40";
    case "CANCELLED":
      return "bg-slate-700/50 text-slate-400 border border-slate-600";
  }
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatArtifact(content: unknown): string {
  if (typeof content === "string") return content;
  return JSON.stringify(content, null, 2);
}
</script>
```

---

## 3. 前端實作的關鍵細節

1. **增量渲染游標（`renderedCount`）與 Xterm.js 解耦**：
   一般新手將終端機輸出綁定在 Vue 的 `v-for` DOM 節點上，當 CLI 吐出超過 2,000 行文字時瀏覽器就會嚴重掉幀。這裡將 Xterm.js 實例獨立於 Vue 的虛擬 DOM 之外，僅透過 `watch(() => store.activeLogs.length)` 取出最新一個 50ms 批次的切片並呼叫 `term.writeln()`，即使處理數萬行編譯輸出依然能維持 60 FPS。
2. **斷線重連自動復原訂閱（Auto-Resubscribe）**：
   當後端服務重啟或網路瞬斷時，`useWebSocket` 的 `onConnected` 鉤子會自動檢查 `activeTaskId.value`，並重新向 Rust 後端發送 `SUBSCRIBE_TASK`，使用者完全不需要手動重新整理網頁即可無縫接續觀看輸出。
3. **頻道色彩分流（Channel Highlighting）**：
   Rust 後端傳來的 `StreamChannel` 分為 `stdout`、`stderr` 與 `system`。前端在寫入 Xterm 時，保留了 `stdout` 的原生 ANSI 色碼，但將後端主動注入的系統狀態（如 PID 啟動、超時強制終止、Artifact 捕獲通知）加上青藍色 `[SYSTEM]` 標籤，讓使用者能一眼區分「CLI 本身的輸出」與「中繼控管系統的動作」。

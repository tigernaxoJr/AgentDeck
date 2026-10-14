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
          {{ store.metrics.runningTasks }}/{{ store.metrics.maxConcurrency }} 執行中
        </span>
      </div>

      <!-- API Key 設定列 -->
      <div class="px-4 py-2 bg-slate-900/80 border-b border-slate-800/80 flex items-center gap-2">
        <span class="text-[11px] font-mono text-slate-400 shrink-0">🔑 Key:</span>
        <input
          v-model="store.apiKey"
          type="password"
          placeholder="可選 API Key..."
          class="w-full bg-slate-950 border border-slate-800 rounded px-2 py-1 text-[11px] font-mono text-slate-200 focus:outline-none focus:border-indigo-500"
        />
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
          class="w-full text-left p-3.5 transition-colors hover:bg-slate-800/50 flex flex-col gap-1.5 cursor-pointer"
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

    <!-- 右側主工作區：指令輸入 + 終端機 / 結構化結果 / Git Diff 檢視器 -->
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
              placeholder="例如: ."
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

      <!-- 中間控制列：分頁切換與中止按鈕 -->
      <header
        class="px-4 py-2 bg-slate-900 border-b border-slate-800 flex items-center justify-between"
      >
        <div class="flex items-center gap-2">
          <button
            @click="activeTab = 'terminal'"
            class="px-3 py-1 text-xs font-medium rounded transition-colors cursor-pointer"
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
            class="px-3 py-1 text-xs font-medium rounded transition-colors flex items-center gap-1.5 cursor-pointer"
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
          <button
            @click="openDiffTab"
            class="px-3 py-1 text-xs font-medium rounded transition-colors flex items-center gap-1.5 cursor-pointer"
            :class="
              activeTab === 'diff'
                ? 'bg-slate-800 text-white'
                : 'text-slate-400 hover:text-slate-200'
            "
          >
            <span>🔀 程式碼變更 (Git Diff)</span>
            <span
              v-if="store.activeDiff?.changedFiles?.length"
              class="px-1.5 py-0.2 rounded-full text-[10px] font-mono bg-indigo-500/30 text-indigo-300"
            >
              {{ store.activeDiff.changedFiles.length }}
            </span>
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
            ⏹ 強制中止 (SIGKILL)
          </button>
        </div>
      </header>

      <!-- 底部顯示區：Xterm.js / JSON Artifact / GitDiffViewer -->
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

        <!-- 分頁 3：Git Diff 檢視器 -->
        <GitDiffViewer
          v-if="activeTab === 'diff'"
          :report="store.activeDiff"
          :loading="store.isDiffLoading"
          @refresh="store.activeTaskId && store.fetchTaskDiff(store.activeTaskId)"
          @revert="handleRevertFile"
          @commit="handleCommitChanges"
        />
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
  nextTick,
} from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { useTaskStore } from "@/stores/useTaskStore";
import GitDiffViewer from "@/components/GitDiffViewer.vue";
import type { LogEntry, TaskState } from "@/types/protocol";

const store = useTaskStore();

const isSubmitting = ref(false);
const activeTab = ref<"terminal" | "artifact" | "diff">("terminal");
const form = reactive({
  prompt: "",
  workspacePath: ".",
  timeoutSecs: 600,
  requireJsonSummary: true,
});

const terminalContainer = ref<HTMLElement | null>(null);
let term: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let resizeObserver: ResizeObserver | null = null;
let renderedCount = 0;

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
    disableStdin: true,
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

  resizeObserver = new ResizeObserver(() => {
    if (activeTab.value === "terminal") {
      fitAddon?.fit();
    }
  });
  resizeObserver.observe(terminalContainer.value);
}

function writeLogToXterm(entry: LogEntry) {
  if (!term) return;

  if (entry.channel === "system") {
    term.writeln(`\x1b[36m[SYSTEM]\x1b[0m ${entry.line}`);
  } else if (entry.channel === "stderr") {
    term.writeln(`\x1b[31m${entry.line}\x1b[0m`);
  } else {
    term.writeln(entry.line);
  }
}

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

    if (activeTab.value === "diff" && store.activeTaskId) {
      store.fetchTaskDiff(store.activeTaskId);
    }
  },
);

watch(
  () => store.activeLogs.length,
  (newLen) => {
    if (!term) return;
    const logs = store.activeLogs;

    if (newLen < renderedCount) {
      term.clear();
      for (const entry of logs) {
        writeLogToXterm(entry);
      }
      renderedCount = newLen;
      return;
    }

    for (let i = renderedCount; i < newLen; i++) {
      writeLogToXterm(logs[i]);
    }
    renderedCount = newLen;
  },
);

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
    await nextTick();
    fitAddon?.fit();
  } finally {
    isSubmitting.value = false;
  }
}

function openDiffTab() {
  activeTab.value = "diff";
  if (store.activeTaskId) {
    store.fetchTaskDiff(store.activeTaskId);
  }
}

async function handleRevertFile(filePath: string) {
  if (store.activeTaskId) {
    await store.revertFile(store.activeTaskId, filePath);
  }
}

async function handleCommitChanges(message?: string) {
  if (store.activeTaskId) {
    await store.commitChanges(store.activeTaskId, message);
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

<template>
  <div
    class="flex h-screen w-screen bg-[#090d11] text-zinc-200 font-sans overflow-hidden antialiased select-none"
  >
    <!-- 左側欄：系統指標與任務佇列清單 -->
    <aside class="w-80 border-r border-zinc-800/80 bg-[#0c1015] flex flex-col shrink-0">
      <!-- 頂部標題與連線狀態 -->
      <div
        class="h-14 px-4 border-b border-zinc-800/80 flex items-center justify-between bg-[#0e131a]"
      >
        <div class="flex items-center gap-2.5">
          <div class="relative flex items-center justify-center">
            <span class="h-2 w-2 rounded-full" :class="wsStatusDot"></span>
            <span
              v-if="store.wsStatus === 'OPEN'"
              class="absolute h-3 w-3 rounded-full bg-emerald-500/20 animate-ping"
            ></span>
          </div>
          <div class="flex items-center gap-2">
            <span class="font-semibold text-xs tracking-wider uppercase text-zinc-100 font-mono">
              Antigravity
            </span>
            <span class="text-[10px] px-1.5 py-0.5 font-mono font-medium rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">
              CLI
            </span>
          </div>
        </div>
        <div class="flex items-center gap-1.5 font-mono text-[11px] text-zinc-400 bg-zinc-900/90 px-2 py-0.5 rounded border border-zinc-800">
          <span class="w-1.5 h-1.5 rounded-full" :class="store.metrics.runningTasks > 0 ? 'bg-cyan-400 animate-pulse' : 'bg-zinc-600'"></span>
          <span>{{ store.metrics.runningTasks }}/{{ store.metrics.maxConcurrency }} 執行</span>
        </div>
      </div>

      <!-- API Key 設定列 -->
      <div class="px-3.5 py-2 bg-[#0e131a]/80 border-b border-zinc-800/60 flex items-center gap-2">
        <span class="text-[10px] font-mono text-zinc-500 uppercase tracking-wider shrink-0">API Key</span>
        <input
          v-model="store.apiKey"
          type="password"
          placeholder="可選權杖..."
          class="w-full bg-zinc-950/90 border border-zinc-800/80 rounded px-2.5 py-1 text-[11px] font-mono text-zinc-300 placeholder-zinc-600 focus:outline-none focus:border-cyan-500/70 transition-colors"
        />
      </div>

      <!-- 佇列負載提示列 -->
      <div
        class="px-4 py-2 bg-[#0a0e13] border-b border-zinc-800/50 flex justify-between items-center text-[11px] text-zinc-400 font-mono"
      >
        <span class="flex items-center gap-1.5">
          <span class="h-1.5 w-1.5 rounded-full bg-amber-400"></span>
          <span>佇列排隊</span>
        </span>
        <span class="font-semibold text-amber-400">{{ store.metrics.queuedTasks }}</span>
      </div>

      <!-- 任務歷史與排程清單 -->
      <div class="flex-1 overflow-y-auto divide-y divide-zinc-850 select-text">
        <button
          v-for="task in store.taskList"
          :key="task.taskId"
          @click="store.selectTask(task.taskId)"
          class="w-full text-left p-3.5 transition-colors flex flex-col gap-1.5 cursor-pointer relative group text-zinc-300"
          :class="
            store.activeTaskId === task.taskId
              ? 'bg-zinc-800/50 border-l-2 border-cyan-400 text-white'
              : 'hover:bg-zinc-900/60 hover:text-zinc-200'
          "
        >
          <div class="flex items-center justify-between">
            <span
              class="text-[10px] font-mono font-medium px-1.5 py-0.5 rounded tracking-wide uppercase"
              :class="stateBadgeClass(task.state)"
            >
              {{ task.state }}
            </span>
            <span class="text-[10px] font-mono text-zinc-500">
              {{ formatTime(task.createdAt) }}
            </span>
          </div>

          <p class="text-xs line-clamp-2 font-normal leading-snug break-all text-zinc-200">
            {{ task.prompt }}
          </p>

          <div class="flex items-center justify-between text-[11px] font-mono text-zinc-500 pt-0.5">
            <span class="truncate max-w-[130px] flex items-center gap-1 text-zinc-400" :title="task.workspacePath">
              📁 {{ task.workspacePath }}
            </span>
            <span v-if="task.model" class="truncate max-w-[110px] px-1 rounded bg-zinc-900 text-zinc-400 text-[10px] border border-zinc-800" :title="task.model">
              {{ task.model.replace('-flash-', '-f-').replace('-medium', '-med') }}
            </span>
          </div>

          <!-- Token Usage 徽章 -->
          <div v-if="task.usage && task.usage.totalTokens > 0" class="flex items-center gap-2 pt-0.5 text-[10px] font-mono text-zinc-400">
            <span class="text-emerald-400 font-medium">⚡ {{ task.usage.totalTokens.toLocaleString() }}</span>
            <span class="text-zinc-700">·</span>
            <span class="text-zinc-500">In: {{ task.usage.inputTokens.toLocaleString() }}</span>
            <span class="text-zinc-500">Out: {{ task.usage.outputTokens.toLocaleString() }}</span>
          </div>
        </button>

        <div
          v-if="store.taskList.length === 0"
          class="p-10 text-center text-xs text-zinc-600 font-mono"
        >
          尚無任務紀錄
        </div>
      </div>
    </aside>

    <!-- 右側主工作區：指令派發 + 終端機 / 結構化結果 / Git Diff 檢視器 -->
    <main class="flex-1 flex flex-col min-w-0 bg-[#090d11]">
      <!-- 頂部：指令派發表單 -->
      <section
        class="p-4 bg-[#0c1015] border-b border-zinc-800/80 flex flex-col gap-3"
      >
        <div class="flex flex-wrap gap-3 items-end">
          <!-- Model 選擇器 -->
          <div class="w-64">
            <div class="flex items-center justify-between text-[11px] font-mono text-zinc-400 mb-1">
              <span>執行模型 (Model)</span>
              <button
                type="button"
                @click="store.fetchModels"
                :disabled="store.isModelsLoading"
                class="text-[10px] text-cyan-400 hover:text-cyan-300 cursor-pointer"
                title="重新整理模型清單"
              >
                {{ store.isModelsLoading ? '載入中...' : '↻ 整理' }}
              </button>
            </div>
            <select
              v-model="form.model"
              class="w-full bg-zinc-950 border border-zinc-800 rounded px-2.5 py-1.5 text-xs font-mono text-zinc-200 focus:outline-none focus:border-cyan-500 cursor-pointer transition-colors"
            >
              <option value="">預設模型 (CLI Default)</option>
              <option
                v-for="m in store.availableModels"
                :key="m.id"
                :value="m.id"
              >
                {{ m.name }} ({{ m.id }})
              </option>
            </select>
          </div>

          <!-- 目標工作目錄 (下拉選單) -->
          <div class="w-64">
            <div class="flex items-center justify-between text-[11px] font-mono text-zinc-400 mb-1">
              <span>工作目錄 (Workspace)</span>
              <button
                type="button"
                @click="store.fetchWorkspaces"
                :disabled="store.isWorkspacesLoading"
                class="text-[10px] text-cyan-400 hover:text-cyan-300 cursor-pointer"
                title="重新整理目錄清單"
              >
                {{ store.isWorkspacesLoading ? '載入中...' : '↻ 整理' }}
              </button>
            </div>
            <select
              v-model="form.workspacePath"
              class="w-full bg-zinc-950 border border-zinc-800 rounded px-2.5 py-1.5 text-xs font-mono text-zinc-200 focus:outline-none focus:border-cyan-500 cursor-pointer transition-colors"
            >
              <option value=".">當前工作目錄 (.)</option>
              <option
                v-for="ws in store.availableWorkspaces"
                :key="ws.path"
                :value="ws.path"
              >
                {{ ws.name }}
              </option>
            </select>
          </div>

          <!-- 超時限制 -->
          <div class="w-28">
            <label class="block text-[11px] font-mono text-zinc-400 mb-1">超時 (秒)</label>
            <input
              v-model.number="form.timeoutSecs"
              type="number"
              min="30"
              max="3600"
              class="w-full bg-zinc-950 border border-zinc-800 rounded px-2.5 py-1.5 text-xs font-mono text-zinc-200 focus:outline-none focus:border-cyan-500 transition-colors"
            />
          </div>

          <!-- 勾選結構化摘要 -->
          <div class="flex items-center pb-2">
            <label
              class="flex items-center gap-2 text-xs text-zinc-300 hover:text-white cursor-pointer select-none"
            >
              <input
                v-model="form.requireJsonSummary"
                type="checkbox"
                class="rounded bg-zinc-950 border-zinc-800 text-cyan-600 focus:ring-0"
              />
              <span>要求結構化 JSON</span>
            </label>
          </div>
        </div>

        <div class="flex gap-3">
          <textarea
            v-model="form.prompt"
            rows="2"
            placeholder="輸入要交給 Antigravity CLI (agy) 執行的指令或 Prompt (Ctrl + Enter 發送)..."
            @keydown.ctrl.enter.prevent="submitTask"
            class="flex-1 bg-zinc-950 border border-zinc-800 rounded-md px-3.5 py-2 text-sm text-zinc-100 placeholder-zinc-600 focus:outline-none focus:border-cyan-500 resize-none transition-colors select-text font-mono"
          ></textarea>
          <button
            @click="submitTask"
            :disabled="isSubmitting || !form.prompt.trim()"
            class="px-5 bg-cyan-600 hover:bg-cyan-500 disabled:bg-zinc-800 disabled:text-zinc-600 text-white font-medium text-xs rounded-md transition-all flex items-center justify-center gap-2 shrink-0 cursor-pointer shadow-sm active:scale-[0.98]"
          >
            <span>🚀 派發任務</span>
          </button>
        </div>

        <!-- 錯誤提示橫幅 -->
        <div
          v-if="store.lastError"
          class="px-3 py-2 rounded bg-rose-950/80 border border-rose-800/80 text-rose-300 text-xs font-mono flex justify-between items-center"
        >
          <span>{{ store.lastError }}</span>
          <button
            @click="store.lastError = null"
            class="text-rose-400 hover:text-white cursor-pointer"
          >
            ✕
          </button>
        </div>
      </section>

      <!-- 中間控制列：分頁切換、使用量儀表與中止按鈕 -->
      <header
        class="h-11 px-4 bg-[#0c1015] border-b border-zinc-800/80 flex items-center justify-between"
      >
        <div class="flex items-center gap-1">
          <button
            @click="activeTab = 'terminal'"
            class="px-3 py-1 text-xs font-medium rounded transition-colors cursor-pointer flex items-center gap-1.5"
            :class="
              activeTab === 'terminal'
                ? 'bg-zinc-800 text-zinc-100'
                : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-850'
            "
          >
            <span>🖥️ 即時終端機</span>
          </button>
          <button
            @click="activeTab = 'artifact'"
            class="px-3 py-1 text-xs font-medium rounded transition-colors flex items-center gap-1.5 cursor-pointer"
            :class="
              activeTab === 'artifact'
                ? 'bg-zinc-800 text-zinc-100'
                : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-850'
            "
          >
            <span>📦 結構化產出</span>
            <span
              v-if="store.activeArtifact"
              class="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse"
            ></span>
          </button>
          <button
            @click="openDiffTab"
            class="px-3 py-1 text-xs font-medium rounded transition-colors flex items-center gap-1.5 cursor-pointer"
            :class="
              activeTab === 'diff'
                ? 'bg-zinc-800 text-zinc-100'
                : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-850'
            "
          >
            <span>🔀 程式碼變更</span>
            <span
              v-if="store.activeDiff?.changedFiles?.length"
              class="px-1.5 py-0.2 rounded text-[10px] font-mono bg-cyan-500/20 text-cyan-300"
            >
              {{ store.activeDiff.changedFiles.length }}
            </span>
          </button>
        </div>

        <!-- 當前任務：Model 與 Token 使用量即時指標 -->
        <div v-if="store.activeTask" class="flex items-center gap-3">
          <!-- Token 使用量 -->
          <div
            v-if="store.activeTask.usage && store.activeTask.usage.totalTokens > 0"
            class="hidden md:flex items-center gap-3 px-3 py-1 bg-zinc-950 border border-zinc-800 rounded text-[11px] font-mono"
          >
            <div class="flex items-center gap-1 text-zinc-300">
              <span class="text-emerald-400">⚡ 總計:</span>
              <span class="font-semibold text-zinc-100">{{ store.activeTask.usage.totalTokens.toLocaleString() }}</span>
            </div>
            <span class="text-zinc-700">|</span>
            <div class="flex items-center gap-1 text-zinc-400">
              <span>輸入:</span>
              <span class="text-zinc-300">{{ store.activeTask.usage.inputTokens.toLocaleString() }}</span>
            </div>
            <div class="flex items-center gap-1 text-zinc-400">
              <span>輸出:</span>
              <span class="text-zinc-300">{{ store.activeTask.usage.outputTokens.toLocaleString() }}</span>
            </div>
            <div v-if="store.activeTask.usage.thinkingTokens > 0" class="flex items-center gap-1 text-zinc-400">
              <span>思考:</span>
              <span class="text-amber-400">{{ store.activeTask.usage.thinkingTokens.toLocaleString() }}</span>
            </div>
          </div>

          <span class="text-xs font-mono text-zinc-500">
            ID: {{ store.activeTask.taskId.slice(0, 8) }}
          </span>

          <button
            v-if="
              store.activeTask.state === 'RUNNING' ||
              store.activeTask.state === 'QUEUED'
            "
            @click="store.abortTask(store.activeTask.taskId)"
            class="px-2.5 py-1 bg-rose-950/80 hover:bg-rose-900 border border-rose-800/80 text-rose-300 hover:text-white text-xs font-mono rounded transition-colors cursor-pointer"
          >
            ⏹ 中止
          </button>
        </div>
      </header>

      <!-- 底部顯示區：Xterm.js / JSON Artifact / GitDiffViewer -->
      <div class="flex-1 relative bg-[#090d11] overflow-hidden">
        <!-- 分頁 1：Xterm.js 終端機容器 -->
        <div
          v-show="activeTab === 'terminal'"
          ref="terminalContainer"
          class="w-full h-full p-2 select-text"
        ></div>

        <!-- 分頁 2：結構化 JSON / Markdown 產出檢視器 -->
        <div
          v-if="activeTab === 'artifact'"
          class="w-full h-full overflow-y-auto p-6 bg-zinc-950 select-text"
        >
          <div v-if="store.activeArtifact" class="max-w-4xl mx-auto space-y-4">
            <div
              class="flex items-center justify-between border-b border-zinc-800 pb-3"
            >
              <div>
                <h2 class="text-sm font-bold text-zinc-200">
                  {{ store.activeArtifact.fileName }}
                </h2>
                <p class="text-xs text-zinc-500 font-mono mt-0.5">
                  Type: {{ store.activeArtifact.artifactType.toUpperCase() }}
                </p>
              </div>
            </div>
            <pre
              class="p-4 rounded bg-[#0c1015] border border-zinc-800 text-xs font-mono text-emerald-300 overflow-x-auto leading-relaxed"
              >{{ formatArtifact(store.activeArtifact.content) }}</pre
            >
          </div>
          <div
            v-else
            class="h-full flex items-center justify-center text-xs text-zinc-600 font-mono"
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
  model: "",
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
  await Promise.all([
    store.fetchTasks(),
    store.fetchModels(),
    store.fetchWorkspaces(),
  ]);
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
    lineHeight: 1.4,
    theme: {
      background: "#090d11",
      foreground: "#d4d4d8",
      cursor: "#06b6d4",
      selectionBackground: "#27272a",
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
        `\x1b[32m[ARTIFACT READY]\x1b[0m 已捕獲結構化結果檔 (${artifact.fileName})，請至上方「結構化產出」分頁檢視。`,
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
      model: form.model.trim() || undefined,
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

const wsStatusDot = computed(() => {
  switch (store.wsStatus) {
    case "OPEN":
      return "bg-emerald-400";
    case "CONNECTING":
      return "bg-amber-400 animate-pulse";
    default:
      return "bg-rose-500";
  }
});

function stateBadgeClass(state: TaskState): string {
  switch (state) {
    case "RUNNING":
      return "bg-cyan-500/10 text-cyan-400 border border-cyan-500/30";
    case "QUEUED":
      return "bg-amber-500/10 text-amber-400 border border-amber-500/30";
    case "COMPLETED":
      return "bg-emerald-500/10 text-emerald-400 border border-emerald-500/30";
    case "FAILED":
    case "TIMEOUT":
      return "bg-rose-500/10 text-rose-400 border border-rose-500/30";
    case "CANCELLED":
      return "bg-zinc-800 text-zinc-500 border border-zinc-700/60";
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

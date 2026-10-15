<template>
  <div class="flex h-full w-full bg-[#090d11] text-zinc-200 overflow-hidden font-mono select-none">
    <!-- 無 Git 儲存庫提示 -->
    <div
      v-if="!report || !report.isGitRepo"
      class="flex-1 flex flex-col items-center justify-center gap-3 text-zinc-500 text-xs"
    >
      <span>⚠️ 當前工作目錄並非 Git 儲存庫，無法進行版本對比與提交。</span>
      <button
        @click="$emit('refresh')"
        class="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-200 text-xs transition-colors cursor-pointer"
      >
        🔄 立即掃描 Git 變更
      </button>
    </div>

    <!-- 主檢視區：左側變更檔案清單 + 右側程式碼差異面板 -->
    <template v-else>
      <!-- 左側：變更檔案統計樹 -->
      <aside
        class="w-72 border-r border-zinc-800/80 bg-[#0c1015] flex flex-col shrink-0 select-text"
      >
        <div
          class="h-11 px-3 border-b border-zinc-800/80 flex items-center justify-between bg-[#0e131a]"
        >
          <div class="flex items-center gap-2 text-xs">
            <span class="text-cyan-400">🌿 {{ report.branch }}</span>
            <span class="px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-300 text-[10px]">
              {{ report.changedFiles.length }} 變更
            </span>
          </div>
          <div class="flex items-center gap-1.5">
            <button
              @click="showCommitModal = true"
              :disabled="report.changedFiles.length === 0"
              class="px-2.5 py-1 rounded bg-cyan-600 hover:bg-cyan-500 disabled:bg-zinc-800 disabled:text-zinc-600 text-white text-[11px] font-medium cursor-pointer transition-colors"
            >
              Commit
            </button>
            <button
              @click="$emit('refresh')"
              :disabled="loading"
              class="text-xs px-2 py-1 rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-300 cursor-pointer"
              title="重新掃描"
            >
              {{ loading ? "..." : "↻" }}
            </button>
          </div>
        </div>

        <div class="flex-1 overflow-y-auto divide-y divide-zinc-800/40">
          <div
            v-for="file in report.changedFiles"
            :key="file.path"
            @click="scrollToFile(file.path)"
            class="w-full px-3 py-2 text-left hover:bg-zinc-850 flex items-center justify-between text-xs transition-colors cursor-pointer group"
          >
            <div class="flex items-center gap-2 truncate">
              <span
                class="w-4 h-4 rounded flex items-center justify-center text-[10px] font-bold shrink-0"
                :class="statusBadgeClass(file.status)"
              >
                {{ file.status }}
              </span>
              <span class="truncate text-zinc-300" :title="file.path">{{
                file.path
              }}</span>
            </div>
            <button
              @click.stop="handleRevert(file.path)"
              class="opacity-0 group-hover:opacity-100 text-[10px] px-1.5 py-0.5 rounded bg-rose-950/80 hover:bg-rose-900 text-rose-300 hover:text-white transition-opacity shrink-0 cursor-pointer"
              title="放棄變更並還原"
            >
              還原
            </button>
          </div>

          <div
            v-if="report.changedFiles.length === 0"
            class="p-6 text-center text-xs text-zinc-600"
          >
            工作樹乾淨 (Working tree clean)
          </div>
        </div>
      </aside>

      <!-- 右側：Unified Diff 分塊渲染區 -->
      <div class="flex-1 overflow-y-auto p-4 space-y-4 text-xs select-text">
        <div
          v-for="block in parsedBlocks"
          :key="block.filePath"
          :id="'diff-file-' + block.filePath"
          class="border border-zinc-800 rounded-md overflow-hidden bg-[#0c1015]"
        >
          <!-- 檔案標頭列 -->
          <div
            class="px-3.5 py-2 bg-[#0e131a] border-b border-zinc-800/80 flex items-center justify-between"
          >
            <span class="font-semibold text-zinc-200">{{ block.filePath }}</span>
            <div class="flex items-center gap-2 text-[11px]">
              <span class="text-emerald-400">+{{ block.additions }}</span>
              <span class="text-rose-400">-{{ block.deletions }}</span>
            </div>
          </div>

          <!-- 逐行程式碼與顏色高亮 -->
          <div class="overflow-x-auto leading-5 py-1">
            <div
              v-for="(line, idx) in block.lines"
              :key="idx"
              class="px-3 whitespace-pre flex"
              :class="lineRowClass(line.type)"
            >
              <span class="w-6 select-none opacity-40 shrink-0 text-zinc-500">{{
                line.prefix
              }}</span>
              <span class="flex-1">{{ line.content }}</span>
            </div>
          </div>
        </div>

        <div
          v-if="parsedBlocks.length === 0"
          class="p-12 text-center text-xs text-zinc-600"
        >
          無程式碼差異可供展示
        </div>
      </div>
    </template>

    <!-- Commit 對話框 -->
    <div
      v-if="showCommitModal"
      class="fixed inset-0 bg-black/75 flex items-center justify-center p-4 z-50 select-text"
    >
      <div
        class="bg-[#0e131a] border border-zinc-800 rounded-md p-5 w-full max-w-md shadow-2xl flex flex-col gap-3 text-xs"
      >
        <h3 class="text-sm font-semibold text-zinc-100">提交 Git 變更 (Commit)</h3>
        <p class="text-zinc-500 text-[11px]">
          將自動加入所有變更檔案並建立版本節點。
        </p>
        <textarea
          v-model="commitMessage"
          rows="3"
          placeholder="留空將自動以任務 Prompt 作為 Commit 訊息..."
          class="w-full bg-zinc-950 border border-zinc-800 rounded p-2 text-zinc-200 focus:outline-none focus:border-cyan-500 resize-none"
        ></textarea>
        <div class="flex justify-end gap-2 pt-2">
          <button
            @click="showCommitModal = false"
            class="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-300 cursor-pointer"
          >
            取消
          </button>
          <button
            @click="handleCommit"
            :disabled="isCommitting"
            class="px-3 py-1.5 rounded bg-cyan-600 hover:bg-cyan-500 text-white font-medium flex items-center gap-1.5 cursor-pointer"
          >
            {{ isCommitting ? "提交中..." : "確認提交" }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from "vue";
import type { GitDiffReport } from "@/types/protocol";

const props = defineProps<{
  report: GitDiffReport | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: "refresh"): void;
  (e: "revert", filePath: string): void;
  (e: "commit", message?: string): void;
}>();

const showCommitModal = ref(false);
const commitMessage = ref("");
const isCommitting = ref(false);

interface DiffLine {
  type: "add" | "del" | "hunk" | "context";
  prefix: string;
  content: string;
}

interface FileDiffBlock {
  filePath: string;
  additions: number;
  deletions: number;
  lines: DiffLine[];
}

const parsedBlocks = computed<FileDiffBlock[]>(() => {
  if (!props.report?.unifiedDiff) return [];

  const rawChunks = props.report.unifiedDiff
    .split(/^diff --git /m)
    .filter(Boolean);

  return rawChunks.map((chunk) => {
    const rawLines = chunk.split("\n");
    const headerLine = rawLines[0] ?? "";
    const match = headerLine.match(/b\/(.+)$/);
    const filePath = match ? match[1] : headerLine;

    let additions = 0;
    let deletions = 0;
    const lines: DiffLine[] = [];

    for (let i = 1; i < rawLines.length; i++) {
      const l = rawLines[i];
      if (
        l.startsWith("+++") ||
        l.startsWith("---") ||
        l.startsWith("index ") ||
        l.startsWith("new file")
      ) {
        continue;
      } else if (l.startsWith("@@")) {
        lines.push({ type: "hunk", prefix: "@@", content: l });
      } else if (l.startsWith("+")) {
        additions++;
        lines.push({ type: "add", prefix: "+", content: l.slice(1) });
      } else if (l.startsWith("-")) {
        deletions++;
        lines.push({ type: "del", prefix: "-", content: l.slice(1) });
      } else {
        lines.push({
          type: "context",
          prefix: " ",
          content: l.startsWith(" ") ? l.slice(1) : l,
        });
      }
    }

    return { filePath, additions, deletions, lines };
  });
});

function scrollToFile(path: string) {
  const el = document.getElementById("diff-file-" + path);
  el?.scrollIntoView({ behavior: "smooth", block: "start" });
}

function handleRevert(path: string) {
  if (confirm(`確定要還原檔案 ${path} 嗎？未提交的修改將永久消失。`)) {
    emit("revert", path);
  }
}

async function handleCommit() {
  isCommitting.value = true;
  try {
    emit("commit", commitMessage.value.trim() || undefined);
    showCommitModal.value = false;
    commitMessage.value = "";
  } finally {
    isCommitting.value = false;
  }
}

function statusBadgeClass(status: string): string {
  switch (status) {
    case "A":
      return "bg-emerald-500/10 text-emerald-400 border border-emerald-500/30";
    case "D":
      return "bg-rose-500/10 text-rose-400 border border-rose-500/30";
    case "R":
      return "bg-cyan-500/10 text-cyan-400 border border-cyan-500/30";
    default:
      return "bg-amber-500/10 text-amber-400 border border-amber-500/30";
  }
}

function lineRowClass(type: DiffLine["type"]): string {
  switch (type) {
    case "add":
      return "bg-emerald-950/40 text-emerald-300";
    case "del":
      return "bg-rose-950/40 text-rose-300";
    case "hunk":
      return "bg-zinc-800/60 text-cyan-300/80 py-0.5 my-1 border-y border-zinc-800";
    default:
      return "text-zinc-400";
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-2xl font-black">AI 調用紀錄與 Prompt 觀察 (Debug 模式)</h2>
        <p class="text-sm text-stone-500 mt-1">即時查看每次送給 AI 的完整 Prompt、學生作答上下文、與 AI 的原始輸出。</p>
      </div>
      <button
        type="button"
        class="px-5 py-2.5 rounded-xl bg-amber-600 text-white font-bold hover:bg-amber-700 flex items-center gap-2 cursor-pointer"
        :disabled="loading"
        @click="load"
      >
        <span>🔄 重新整理</span>
      </button>
    </div>

    <div v-if="loading && calls.length === 0" class="text-xl text-stone-400 py-8 text-center">
      載入中…
    </div>

    <div v-else-if="calls.length === 0" class="bg-white dark:bg-stone-850 rounded-3xl p-8 text-center text-stone-500 border border-stone-200 dark:border-stone-700">
      目前尚無 AI 調用紀錄。
    </div>

    <div v-else class="flex flex-col gap-4">
      <div
        v-for="call in calls"
        :key="call.id"
        class="bg-white dark:bg-stone-850 rounded-3xl p-6 shadow-sm border border-stone-200 dark:border-stone-700 flex flex-col gap-4"
      >
        <!-- Header -->
        <div class="flex flex-wrap items-center justify-between gap-3 pb-3 border-b border-stone-100 dark:border-stone-800">
          <div class="flex items-center gap-3">
            <span
              :class="[
                'px-3 py-1 rounded-full text-sm font-black',
                call.ok ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300' : 'bg-rose-100 text-rose-800 dark:bg-rose-950 dark:text-rose-300'
              ]"
            >
              {{ call.ok ? '✅ 成功' : '❌ 失敗' }}
            </span>
            <span class="font-mono text-sm px-2.5 py-1 rounded-lg bg-stone-100 dark:bg-stone-800 text-stone-700 dark:text-stone-300">
              {{ call.backend }}
            </span>
            <span class="font-bold text-stone-800 dark:text-stone-200">
              用途：{{ purposeLabel(call.purpose) }}
            </span>
            <span v-if="call.questionId" class="text-xs text-stone-400 font-mono">
              題目: {{ call.questionId }}
            </span>
          </div>

          <div class="text-xs text-stone-400 flex items-center gap-4">
            <span v-if="call.durationMs">耗時: {{ call.durationMs }}ms</span>
            <span>{{ formatTime(call.createdAt) }}</span>
          </div>
        </div>

        <!-- Prompt Section -->
        <div class="flex flex-col gap-2">
          <div class="flex items-center justify-between">
            <span class="text-sm font-bold text-amber-700 dark:text-amber-400 flex items-center gap-1.5">
              <span>📝 送給 AI 的完整 Prompt</span>
            </span>
            <button
              type="button"
              class="text-xs text-stone-500 hover:text-amber-600 underline cursor-pointer"
              @click="toggleExpand(call.id, 'prompt')"
            >
              {{ isExpanded(call.id, 'prompt') ? '收合' : '展開完整內容' }}
            </button>
          </div>
          <pre
            :class="[
              'p-4 rounded-2xl bg-stone-50 dark:bg-stone-900 border border-stone-200 dark:border-stone-800 text-xs font-mono whitespace-pre-wrap break-all text-stone-800 dark:text-stone-200 overflow-hidden transition-all',
              isExpanded(call.id, 'prompt') ? 'max-h-none' : 'max-h-40'
            ]"
          >{{ call.prompt || '(未記錄 Prompt)' }}</pre>
        </div>

        <!-- Output Section -->
        <div class="flex flex-col gap-2">
          <div class="flex items-center justify-between">
            <span class="text-sm font-bold text-stone-700 dark:text-stone-300 flex items-center gap-1.5">
              <span>🤖 AI 原始輸出 (turn.json / Response)</span>
            </span>
            <button
              type="button"
              class="text-xs text-stone-500 hover:text-amber-600 underline cursor-pointer"
              @click="toggleExpand(call.id, 'output')"
            >
              {{ isExpanded(call.id, 'output') ? '收合' : '展開完整內容' }}
            </button>
          </div>
          <pre
            :class="[
              'p-4 rounded-2xl bg-stone-900 text-stone-100 text-xs font-mono whitespace-pre-wrap break-all overflow-hidden transition-all',
              isExpanded(call.id, 'output') ? 'max-h-none' : 'max-h-36'
            ]"
          >{{ call.rawOutput || call.error || '(無輸出)' }}</pre>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import * as api from './api';

const calls = ref<any[]>([]);
const loading = ref(false);
const expanded = ref<Record<string, boolean>>({});

function isExpanded(id: string, type: string) {
  return !!expanded.value[`${id}_${type}`];
}

function toggleExpand(id: string, type: string) {
  const k = `${id}_${type}`;
  expanded.value[k] = !expanded.value[k];
}

function purposeLabel(p: string) {
  switch (p) {
    case 'tutor_wrong':
      return '引導答錯題目 (TutorWrong)';
    case 'grade_handwriting':
      return '手寫/畫布批改 (GradeHandwriting)';
    case 'student_message':
      return '學生主動發問 (StudentMessage)';
    case 'enrich_page':
      return '題庫頁碼補強 (EnrichPage)';
    default:
      return p;
  }
}

function formatTime(t: string) {
  try {
    const d = new Date(t);
    return d.toLocaleString('zh-TW', { hour12: false });
  } catch {
    return t;
  }
}

async function load() {
  loading.value = true;
  try {
    calls.value = await api.fetchAiCalls();
  } catch (e: any) {
    alert(e.message || '載入失敗');
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  load();
});
</script>

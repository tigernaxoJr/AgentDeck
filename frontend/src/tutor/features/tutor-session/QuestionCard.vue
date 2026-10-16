<template>
  <div class="bg-white dark:bg-stone-800 rounded-3xl p-6 shadow-sm border-2 border-stone-200 dark:border-stone-700 flex flex-col gap-4">
    <div class="flex items-center gap-3 flex-wrap text-base font-bold">
      <span class="bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300 px-3 py-1 rounded-full">
        {{ typeLabel }}
      </span>
      <span v-if="question.section && !question.section.includes(typeLabel)" class="text-stone-500 dark:text-stone-400">
        {{ question.section }}
      </span>
      <span v-if="question.bookPage" class="ml-auto text-stone-500 dark:text-stone-400">
        📖 課本第 {{ question.bookPage }} 頁<template v-if="question.bookWhere"> · {{ question.bookWhere }}</template>
      </span>
    </div>

    <div :class="['flex flex-col gap-3 font-bold text-stone-900 dark:text-stone-100 leading-relaxed tracking-wide', stemSize]">
      <template v-for="(block, i) in stemBlocks" :key="i">
        <p v-if="block.kind === 'text'" class="whitespace-pre-line">{{ block.text }}</p>
        <div v-else class="overflow-x-auto">
          <table class="border-collapse text-[0.8em] text-center">
            <thead>
              <tr>
                <th
                  v-for="(cell, j) in block.head"
                  :key="j"
                  class="border-2 border-stone-300 dark:border-stone-600 bg-amber-50 dark:bg-stone-700 px-4 py-2"
                >
                  {{ cell }}
                </th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(row, r) in block.rows" :key="r">
                <td
                  v-for="(cell, j) in row"
                  :key="j"
                  :class="['border-2 border-stone-300 dark:border-stone-600 px-4 py-2', j === 0 && 'bg-amber-50 dark:bg-stone-700']"
                >
                  {{ cell }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </template>
    </div>

    <!-- 圖上作答時，圖片在作答區顯示，這裡不重複 -->
    <button
      v-if="question.imageUrl && question.inputMode !== 'image_canvas'"
      type="button"
      class="group relative self-start cursor-zoom-in"
      @click="showEnlarged = true"
    >
      <img
        :src="question.imageUrl"
        class="max-h-72 rounded-2xl border-2 border-stone-200 dark:border-stone-600 object-contain bg-white"
        alt="題目附圖"
      />
      <span class="absolute bottom-2 right-2 rounded-full bg-black/60 text-white text-base px-3 py-1 opacity-80 group-hover:opacity-100">
        🔍 點我放大
      </span>
    </button>

    <div
      v-if="showEnlarged"
      class="fixed inset-0 z-50 bg-black/80 flex items-center justify-center p-6 cursor-zoom-out"
      @click="showEnlarged = false"
    >
      <img :src="question.imageUrl" class="max-w-full max-h-full rounded-2xl shadow-2xl bg-white" alt="題目附圖（放大）" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue';
import type { QuestionView } from '../../shared/types';

const props = defineProps<{
  question: QuestionView;
}>();

const showEnlarged = ref(false);

const TYPE_LABELS: Record<string, string> = {
  true_false: '是非題',
  single_choice: '選擇題',
  fill_blank: '填空題',
  short_answer: '簡答題',
};

const typeLabel = computed(() => TYPE_LABELS[props.question.qtype] ?? '練習題');

type StemBlock =
  | { kind: 'text'; text: string }
  | { kind: 'table'; head: string[]; rows: string[][] };

const splitRow = (line: string) =>
  line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim());

const isDivider = (line: string) => /^\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?$/.test(line.trim());

/** 題幹可能夾帶 Markdown 表格（| a | b |），其餘保留換行 */
const stemBlocks = computed<StemBlock[]>(() => {
  const blocks: StemBlock[] = [];
  const text: string[] = [];
  const flushText = () => {
    const t = text.join('\n').trim();
    if (t) blocks.push({ kind: 'text', text: t });
    text.length = 0;
  };

  const lines = props.question.stem.split('\n');
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].trim().startsWith('|')) {
      text.push(lines[i]);
      continue;
    }
    const tableLines: string[] = [];
    while (i < lines.length && lines[i].trim().startsWith('|')) tableLines.push(lines[i++]);
    i--;
    const rows = tableLines.filter((l) => !isDivider(l)).map(splitRow);
    flushText();
    blocks.push({ kind: 'table', head: rows[0] ?? [], rows: rows.slice(1) });
  }
  flushText();
  return blocks;
});

/** 題幹越長字越小，避免長題目把作答區擠出畫面 */
const stemSize = computed(() => {
  const len = stemBlocks.value.reduce((n, b) => n + (b.kind === 'text' ? b.text.length : 0), 0);
  if (len <= 40) return 'text-3xl md:text-4xl';
  if (len <= 100) return 'text-2xl md:text-3xl';
  return 'text-xl md:text-2xl';
});
</script>

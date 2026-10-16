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
        📖 課本第 {{ question.bookPage }} 頁
      </span>
    </div>

    <h2 class="text-3xl md:text-4xl font-bold text-stone-900 dark:text-stone-100 leading-relaxed tracking-wide">
      {{ question.stem }}
    </h2>

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
  handwrite_blank: '填空題',
  handwrite_short_answer: '簡答題',
};

const typeLabel = computed(() => TYPE_LABELS[props.question.qtype] ?? '練習題');
</script>

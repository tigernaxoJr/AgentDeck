<template>
  <div class="bg-white dark:bg-stone-800 rounded-3xl p-6 shadow-md border-2 border-stone-200 dark:border-stone-700 flex flex-col gap-4">
    <div class="flex items-center justify-between text-stone-500 dark:text-stone-400 font-medium text-lg">
      <span>{{ question.section || '練習題目' }}</span>
      <span v-if="question.bookPage" class="bg-stone-100 dark:bg-stone-700 px-3 py-1 rounded-full text-base">
        課本第 {{ question.bookPage }} 頁
      </span>
    </div>

    <!-- Stem (Large font for eye care) -->
    <h2 class="text-3xl md:text-4xl font-bold text-stone-900 dark:text-stone-100 leading-relaxed tracking-wide">
      {{ question.stem }}
    </h2>

    <!-- Question Image Thumbnail (Click to enlarge) -->
    <div v-if="question.imageUrl" class="mt-2">
      <img
        :src="question.imageUrl"
        class="max-h-64 rounded-2xl border-2 border-stone-200 dark:border-stone-600 object-contain cursor-pointer hover:scale-102 transition-transform"
        alt="Question thumbnail"
        @click="showEnlarged = true"
      />
    </div>

    <!-- Modal for enlarged image -->
    <div
      v-if="showEnlarged"
      class="fixed inset-0 z-50 bg-black/80 flex items-center justify-center p-4 cursor-pointer"
      @click="showEnlarged = false"
    >
      <img
        :src="question.imageUrl"
        class="max-w-full max-h-full rounded-2xl shadow-2xl"
        alt="Enlarged"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import type { QuestionView } from '../../shared/types';

defineProps<{
  question: QuestionView;
}>();

const showEnlarged = ref(false);
</script>

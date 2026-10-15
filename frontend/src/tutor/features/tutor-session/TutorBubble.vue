<template>
  <div class="flex flex-col gap-4 overflow-y-auto max-h-[500px] p-2 pr-4">
    <div
      v-for="(item, idx) in dialog"
      :key="idx"
      :class="[
        'flex flex-col p-5 rounded-3xl max-w-[85%] text-xl leading-relaxed shadow-sm',
        item.role === 'tutor'
          ? 'self-start bg-amber-50 dark:bg-stone-800 border-2 border-amber-200 dark:border-stone-700 text-stone-900 dark:text-stone-100'
          : 'self-end bg-stone-200 dark:bg-stone-700 text-stone-900 dark:text-stone-100'
      ]"
    >
      <div class="flex items-center gap-2 mb-1 font-bold text-base text-stone-500 dark:text-stone-400">
        <span v-if="item.role === 'tutor'">🧑‍🏫 家教老師</span>
        <span v-else>👦 我</span>
      </div>
      <div>{{ item.content }}</div>

      <!-- Book ref tag inside bubble if present -->
      <div v-if="item.bookRef" class="mt-2 text-base font-bold text-amber-700 dark:text-amber-300">
        📖 翻到課本第 {{ item.bookRef.page }} 頁 {{ item.bookRef.where }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { DialogTurnView } from '../../shared/types';

defineProps<{
  dialog: DialogTurnView[];
}>();
</script>

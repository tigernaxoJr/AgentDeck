<template>
  <div
    v-if="bookRef"
    class="bg-amber-100 dark:bg-amber-950/60 border-3 border-amber-500 rounded-2xl p-4 flex items-center gap-4 text-amber-950 dark:text-amber-100 shadow-md animate-pulse"
  >
    <div class="text-4xl">📖</div>
    <div class="flex flex-col">
      <span class="text-xl font-bold">請翻開實體課本 / 評量</span>
      <span class="text-2xl font-black text-amber-800 dark:text-amber-300">
        第 {{ bookRef.page }} 頁 {{ bookRef.where }}
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { BookRef } from '../../shared/types';

defineProps<{
  bookRef?: BookRef;
}>();
</script>

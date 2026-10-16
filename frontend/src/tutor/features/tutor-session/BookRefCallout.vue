<template>
  <div
    v-if="bookRef"
    :key="`${bookRef.page}-${bookRef.where}`"
    class="callout-pop bg-amber-50 dark:bg-amber-950/50 border-l-8 border-amber-500 rounded-2xl px-5 py-4 flex items-center gap-4 text-amber-950 dark:text-amber-100"
  >
    <div class="text-4xl">📖</div>
    <div class="flex flex-col">
      <span class="text-lg font-bold text-amber-700 dark:text-amber-400">請翻開課本 / 評量</span>
      <span class="text-2xl font-black">
        第 {{ bookRef.page }} 頁<span v-if="bookRef.where" class="font-bold"> · {{ bookRef.where }}</span>
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

<style scoped>
/* 頁碼改變時閃一下提醒，不持續閃爍 */
.callout-pop {
  animation: callout-pop 0.9s ease-out;
}
@keyframes callout-pop {
  0% { transform: scale(0.97); box-shadow: 0 0 0 0 rgb(245 158 11 / 0.6); }
  50% { transform: scale(1.01); box-shadow: 0 0 0 10px rgb(245 158 11 / 0); }
  100% { transform: none; }
}
@media (prefers-reduced-motion: reduce) {
  .callout-pop { animation: none; }
}
</style>

<template>
  <div ref="scrollRef" class="flex flex-col gap-4 overflow-y-auto p-1 pr-2 scroll-smooth">
    <div
      v-if="dialog.length === 0 && !thinking"
      class="m-auto flex flex-col items-center gap-3 text-center text-stone-400 dark:text-stone-500 py-8"
    >
      <span class="text-5xl">🧑‍🏫</span>
      <p class="text-xl leading-relaxed">先試著自己作答吧！<br />答錯或有問題時，老師會在這裡陪你。</p>
    </div>

    <div
      v-for="(item, idx) in dialog"
      :key="idx"
      :class="['flex gap-3 max-w-[92%] bubble-in', item.role === 'tutor' ? 'self-start' : 'self-end flex-row-reverse']"
    >
      <span class="shrink-0 w-11 h-11 rounded-full flex items-center justify-center text-2xl bg-stone-100 dark:bg-stone-800">
        {{ item.role === 'tutor' ? '🧑‍🏫' : '🙂' }}
      </span>
      <div
        :class="[
          'px-5 py-4 rounded-3xl text-xl leading-relaxed whitespace-pre-wrap',
          item.role === 'tutor'
            ? 'rounded-tl-md bg-amber-50 border-2 border-amber-200 text-stone-900 dark:bg-stone-800 dark:border-stone-700 dark:text-stone-100'
            : 'rounded-tr-md bg-amber-600 text-white',
        ]"
      >
        {{ item.content }}
        <div
          v-if="item.bookRef"
          class="mt-3 inline-flex items-center gap-2 rounded-xl bg-amber-200/70 dark:bg-amber-900/50 px-3 py-1.5 text-lg font-bold text-amber-900 dark:text-amber-200"
        >
          📖 翻到第 {{ item.bookRef.page }} 頁 {{ item.bookRef.where }}
        </div>
      </div>
    </div>

    <div v-if="thinking" class="flex gap-3 self-start bubble-in">
      <span class="shrink-0 w-11 h-11 rounded-full flex items-center justify-center text-2xl bg-stone-100 dark:bg-stone-800">🧑‍🏫</span>
      <div class="px-5 py-4 rounded-3xl rounded-tl-md bg-amber-50 border-2 border-amber-200 dark:bg-stone-800 dark:border-stone-700 flex items-center gap-3 text-lg text-stone-500 dark:text-stone-400">
        <span class="flex gap-1.5">
          <span class="dot" /><span class="dot [animation-delay:150ms]" /><span class="dot [animation-delay:300ms]" />
        </span>
        {{ thinkingText }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, onUnmounted } from 'vue';
import type { DialogTurnView } from '../../shared/types';

const props = defineProps<{
  dialog: DialogTurnView[];
  thinking?: boolean;
}>();

const scrollRef = ref<HTMLDivElement | null>(null);

/** AI 批改可能要二、三十秒，依等待時間換句話，讓孩子知道還在處理 */
const waitedSec = ref(0);
let waitTimer: ReturnType<typeof setInterval> | undefined;

watch(
  () => props.thinking,
  (on) => {
    clearInterval(waitTimer);
    waitedSec.value = 0;
    if (on) waitTimer = setInterval(() => waitedSec.value++, 1000);
  },
  { immediate: true }
);
onUnmounted(() => clearInterval(waitTimer));

const thinkingText = computed(() => {
  if (waitedSec.value < 6) return '老師想一下…';
  if (waitedSec.value < 15) return '老師正在仔細看你的答案…';
  return '快好了，再等一下下…';
});

watch(
  () => [props.dialog.length, props.thinking],
  async () => {
    await nextTick();
    scrollRef.value?.scrollTo({ top: scrollRef.value.scrollHeight });
  },
  { immediate: true }
);
</script>

<style scoped>
.dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 9999px;
  background: #d97706;
  animation: dot-bounce 1s infinite ease-in-out;
}
@keyframes dot-bounce {
  0%, 80%, 100% { transform: translateY(0); opacity: 0.4; }
  40% { transform: translateY(-5px); opacity: 1; }
}
.bubble-in {
  animation: bubble-in 0.25s ease-out;
}
@keyframes bubble-in {
  from { opacity: 0; transform: translateY(8px); }
  to { opacity: 1; transform: none; }
}
@media (prefers-reduced-motion: reduce) {
  .dot, .bubble-in { animation: none; }
}
</style>

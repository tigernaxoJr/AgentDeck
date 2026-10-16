<template>
  <div class="h-screen overflow-hidden bg-[#faf6f0] dark:bg-[#1c1917] text-stone-900 dark:text-stone-100 flex flex-col">
    <!-- Header -->
    <header class="shrink-0 flex items-center gap-6 px-6 py-4 border-b-2 border-stone-200 dark:border-stone-800">
      <h1 class="text-2xl font-black text-amber-700 dark:text-amber-500 whitespace-nowrap">AI 家教複習</h1>

      <div v-if="session && !session.empty" class="flex-1 flex items-center gap-4 min-w-0">
        <div class="flex-1 h-4 rounded-full bg-stone-200 dark:bg-stone-800 overflow-hidden max-w-xl">
          <div
            class="h-full rounded-full bg-amber-500 transition-all duration-500"
            :style="{ width: `${progressPct}%` }"
          />
        </div>
        <span class="text-xl font-bold whitespace-nowrap">
          第 {{ session.index }} <span class="text-stone-400">/ {{ session.total }}</span> 題
        </span>
      </div>
      <div v-else class="flex-1" />

      <button
        v-if="!summary"
        type="button"
        class="px-5 py-2.5 rounded-2xl bg-stone-200 hover:bg-stone-300 dark:bg-stone-800 dark:hover:bg-stone-700 font-bold text-lg cursor-pointer"
        @click="session?.empty ? router.push('/tutor') : end()"
      >
        結束複習
      </button>
    </header>

    <!-- Finished -->
    <main v-if="summary" class="flex-1 flex items-center justify-center p-8 overflow-y-auto">
      <div class="bg-white dark:bg-stone-800 rounded-[2rem] shadow-sm border-2 border-stone-200 dark:border-stone-700 p-10 max-w-2xl w-full flex flex-col items-center gap-8 text-center">
        <div class="text-7xl">🎉</div>
        <h2 class="text-4xl font-black">這次複習完成了！</h2>
        <div class="grid grid-cols-2 gap-4 w-full">
          <div class="rounded-3xl bg-amber-50 dark:bg-amber-950/40 p-6">
            <div class="text-5xl font-black text-amber-600">{{ summary.answered }}</div>
            <div class="text-xl font-bold text-stone-600 dark:text-stone-300 mt-1">練習題數</div>
          </div>
          <div class="rounded-3xl bg-emerald-50 dark:bg-emerald-950/40 p-6">
            <div class="text-5xl font-black text-emerald-600">{{ summary.firstTryCorrect }}</div>
            <div class="text-xl font-bold text-stone-600 dark:text-stone-300 mt-1">一次就答對</div>
          </div>
        </div>
        <div v-if="summary.weakConcepts.length" class="w-full text-left">
          <div class="text-xl font-bold mb-3">下次再多看看這些：</div>
          <div class="flex flex-wrap gap-2">
            <span
              v-for="c in summary.weakConcepts"
              :key="c"
              class="px-4 py-2 rounded-full bg-stone-100 dark:bg-stone-700 text-lg font-bold"
            >
              {{ c }}
            </span>
          </div>
        </div>
        <BigButton variant="primary" class="w-full" @click="router.push('/tutor')">回首頁</BigButton>
      </div>
    </main>

    <!-- Empty queue -->
    <main v-else-if="session?.empty" class="flex-1 flex flex-col items-center justify-center gap-6 p-8 text-center">
      <div class="text-7xl">🌟</div>
      <p class="text-3xl font-bold">{{ session.message || '目前沒有需要複習的題目喔！' }}</p>
      <BigButton variant="primary" @click="router.push('/tutor')">回首頁</BigButton>
    </main>

    <!-- Session -->
    <main v-else-if="session?.question" class="flex-1 min-h-0 grid grid-cols-1 lg:grid-cols-12 gap-6 p-6 overflow-y-auto lg:overflow-hidden">
      <!-- Left: question & answer -->
      <section class="lg:col-span-7 xl:col-span-8 flex flex-col lg:min-h-0">
        <div ref="questionScrollRef" class="flex flex-col gap-5 lg:flex-1 lg:min-h-0 lg:overflow-y-auto lg:pr-2 pb-2">
          <QuestionCard :key="session.question.id" :question="session.question" />

          <AnswerPanel
            ref="answerPanelRef"
            :key="session.question.id"
            :question="session.question"
            :disabled="aiThinking || loading"
            :locked="canNext"
            :reply="lastReply"
            @submit="submitAnswer"
          />

          <!-- 批改結果 -->
          <div v-if="feedback" ref="feedbackRef" :key="feedbackKey" class="feedback-in flex flex-col gap-3">
            <div :class="['rounded-2xl border-l-8 px-5 py-4 flex flex-col gap-2', FEEDBACK_TONES[feedback.tone]]">
              <span class="text-2xl font-black">{{ feedback.title }}</span>
              <span v-if="feedback.answer" class="text-3xl font-black">{{ feedback.answer }}</span>
              <p v-if="feedback.explanation" class="text-xl leading-relaxed">{{ feedback.explanation }}</p>
            </div>
            <BookRefCallout v-if="feedback.tone === 'bad'" :book-ref="hintBookRef" />
          </div>
        </div>

        <!-- 作答列：固定在底部，不會被長題目擠出畫面 -->
        <div class="shrink-0 flex items-center justify-between gap-4 pt-4 border-t-2 border-stone-200 dark:border-stone-800">
          <p class="text-lg text-stone-500 dark:text-stone-400">
            {{ canNext ? '按 Enter 繼續' : answerPanelRef?.hint }}
          </p>
          <BigButton v-if="canNext" variant="primary" :disabled="loading" @click="next">
            {{ isLast ? '完成複習 🎉' : '下一題 ➔' }}
          </BigButton>
          <BigButton
            v-else
            variant="primary"
            :disabled="aiThinking || loading || !answerPanelRef?.hasAnswer"
            @click="answerPanelRef?.submit()"
          >
            {{ aiThinking ? '老師批改中…' : '送出答案' }}
          </BigButton>
        </div>
      </section>

      <!-- Right: tutor chat -->
      <aside class="lg:col-span-5 xl:col-span-4 min-h-[420px] lg:min-h-0 flex flex-col bg-white dark:bg-stone-900 rounded-3xl shadow-sm border-2 border-stone-200 dark:border-stone-800 overflow-hidden">
        <div class="px-5 py-3 border-b-2 border-stone-100 dark:border-stone-800 text-lg font-bold text-stone-500 dark:text-stone-400">
          家教老師
        </div>

        <TutorBubble :dialog="session.dialog" :thinking="aiThinking || hintLoading" class="flex-1 min-h-0 px-4 py-4" />

        <form
          class="flex items-center gap-2 p-3 border-t-2 border-stone-100 dark:border-stone-800"
          @submit.prevent="sendStudentMessage"
        >
          <input
            ref="askInputRef"
            v-model="askText"
            type="text"
            class="flex-1 min-w-0 px-4 py-3 text-xl rounded-2xl border-2 border-stone-200 dark:border-stone-700 bg-stone-50 dark:bg-stone-800 focus:border-amber-500 outline-none"
            placeholder="有不懂的地方？問老師… (?)"
            :disabled="aiThinking"
            @keydown.esc="($event.target as HTMLInputElement).blur()"
          />
          <button
            type="submit"
            class="px-5 py-3 rounded-2xl bg-amber-600 hover:bg-amber-700 text-white text-xl font-bold cursor-pointer disabled:opacity-40 disabled:cursor-default"
            :disabled="aiThinking || !askText.trim()"
          >
            發問
          </button>
        </form>
      </aside>
    </main>

    <!-- Loading -->
    <main v-else class="flex-1 flex flex-col items-center justify-center gap-6 text-2xl text-stone-400">
      <template v-if="loading">題目載入中…</template>
      <template v-else>
        <span>題目載入失敗</span>
        <BigButton variant="secondary" @click="load">再試一次</BigButton>
      </template>
    </main>

    <!-- Self Grade Modal -->
    <div
      v-if="selfGradeModal && lastReply?.reveal"
      class="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4"
    >
      <div class="bg-white dark:bg-stone-800 rounded-3xl p-8 max-w-xl w-full shadow-2xl flex flex-col gap-6 text-center">
        <h3 class="text-3xl font-black">自己對答案</h3>
        <div class="p-6 bg-stone-100 dark:bg-stone-900 rounded-2xl text-left">
          <div class="text-lg font-bold text-stone-500 mb-1">正確答案</div>
          <div class="text-3xl font-black text-amber-600 mb-3">{{ lastReply.reveal.answer }}</div>
          <div class="text-lg text-stone-600 dark:text-stone-300">{{ lastReply.reveal.explanation }}</div>
        </div>
        <p class="text-xl font-bold">對照你寫的答案，你覺得答對了嗎？</p>
        <div class="grid grid-cols-2 gap-4">
          <button
            type="button"
            class="py-6 rounded-2xl bg-emerald-600 hover:bg-emerald-700 text-white font-black text-3xl shadow-lg cursor-pointer"
            @click="selfGrade(true)"
          >
            ✅ 我寫對了
          </button>
          <button
            type="button"
            class="py-6 rounded-2xl bg-rose-600 hover:bg-rose-700 text-white font-black text-3xl shadow-lg cursor-pointer"
            @click="selfGrade(false)"
          >
            ❌ 寫錯了
          </button>
        </div>
      </div>
    </div>

    <!-- Error toast -->
    <Transition
      enter-from-class="opacity-0 translate-y-4"
      leave-to-class="opacity-0 translate-y-4"
      enter-active-class="transition duration-200"
      leave-active-class="transition duration-200"
    >
      <div
        v-if="toast"
        role="alert"
        class="fixed bottom-6 left-1/2 -translate-x-1/2 z-50 px-6 py-4 rounded-2xl bg-rose-600 text-white text-xl font-bold shadow-xl"
      >
        {{ toast }}
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useTutorSession } from './useTutorSession';
import QuestionCard from './QuestionCard.vue';
import BookRefCallout from './BookRefCallout.vue';
import TutorBubble from './TutorBubble.vue';
import AnswerPanel from './AnswerPanel.vue';
import BigButton from '../../shared/BigButton.vue';

const route = useRoute();
const router = useRouter();
const sessionId = route.params.id as string;
const answerPanelRef = ref<InstanceType<typeof AnswerPanel> | null>(null);
const askInputRef = ref<HTMLInputElement | null>(null);
const questionScrollRef = ref<HTMLDivElement | null>(null);

const {
  session,
  loading,
  aiThinking,
  hintLoading,
  selfGradeModal,
  lastReply,
  askText,
  summary,
  toast,
  canNext,
  isLast,
  load,
  submitAnswer,
  selfGrade,
  next,
  end,
  sendStudentMessage,
} = useTutorSession(sessionId);

const feedbackRef = ref<HTMLDivElement | null>(null);

type FeedbackTone = 'good' | 'bad' | 'warn' | 'reveal';
const FEEDBACK_TONES: Record<FeedbackTone, string> = {
  good: 'bg-emerald-50 border-emerald-500 text-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-100',
  reveal: 'bg-sky-50 border-sky-500 text-sky-950 dark:bg-sky-950/40 dark:text-sky-100',
  bad: 'bg-rose-50 border-rose-400 text-rose-900 dark:bg-rose-950/40 dark:text-rose-100',
  warn: 'bg-amber-50 border-amber-500 text-amber-900 dark:bg-amber-950/40 dark:text-amber-100',
};

const revealText = computed(() => {
  const r = lastReply.value?.reveal;
  if (!r) return '';
  if (typeof r.choice === 'boolean') return r.choice ? '⭕ 對' : '❌ 錯';
  return r.answer;
});

/** 左側的批改結果卡；老師的完整說明在右側對話 */
const feedback = computed<{ tone: FeedbackTone; title: string; answer?: string; explanation?: string } | null>(() => {
  const r = lastReply.value;
  if (!r) return null;
  const isChoice = session.value?.question?.inputMode === 'choice';
  if (canNext.value) {
    if (r.verdict === 'correct') {
      // 選擇題的正解已在選項上打勾，不再重複
      return { tone: 'good', title: '🎉 答對了！', answer: isChoice ? '' : revealText.value, explanation: r.reveal?.explanation };
    }
    return { tone: 'reveal', title: '💡 正確答案', answer: revealText.value, explanation: r.reveal?.explanation };
  }
  switch (r.verdict) {
    case 'incorrect':
      return { tone: 'bad', title: '還不對喔！看看老師的提示，再試一次 💪' };
    case 'partial':
      return { tone: 'warn', title: '有一部分對了，再想想看還少了什麼！' };
    case 'unreadable':
      return { tone: 'warn', title: '字跡有點看不清楚，寫大一點再試一次！' };
    default:
      return null;
  }
});

/** 每次批改都換 key，重播進場動畫 */
const feedbackKey = ref(0);

/** 答錯時才提示翻課本；題目卡上已標示頁碼 */
const hintBookRef = computed(() => {
  const q = session.value?.question;
  return lastReply.value?.bookRef || (q?.bookPage ? { page: q.bookPage, where: q.bookWhere || '' } : undefined);
});

/** 已完成的題數占比；本題揭曉後算完成 */
const progressPct = computed(() => {
  const s = session.value;
  if (!s || !s.total) return 0;
  const done = s.index - 1 + (canNext.value ? 1 : 0);
  return Math.round((done / s.total) * 100);
});

// 換題時回到頂端，否則會停在上一題的捲動位置
watch(
  () => session.value?.question?.id,
  async () => {
    await nextTick();
    questionScrollRef.value?.scrollTo({ top: 0 });
  }
);

// 批改結果出來時把結果卡捲進畫面
watch(lastReply, async (r) => {
  if (!r) return;
  feedbackKey.value++;
  await nextTick();
  feedbackRef.value?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
});

onMounted(() => {
  load();
  window.addEventListener('keydown', handleGlobalKeydown);
});

onUnmounted(() => {
  window.removeEventListener('keydown', handleGlobalKeydown);
});

function handleGlobalKeydown(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement).tagName.toLowerCase();
  if (['input', 'textarea'].includes(tag) || e.ctrlKey || e.metaKey || e.altKey) return;
  if (summary.value || selfGradeModal.value || !session.value?.question) return;

  const key = e.key.toLowerCase();
  if (e.key === 'Enter') {
    e.preventDefault();
    if (canNext.value) next();
    else answerPanelRef.value?.submit();
  } else if (key === 'c') {
    answerPanelRef.value?.clear();
  } else if (key === 'z') {
    answerPanelRef.value?.undo();
  } else if (e.key === '?') {
    e.preventDefault();
    askInputRef.value?.focus();
  } else if (/^[1-9]$/.test(e.key)) {
    answerPanelRef.value?.selectChoice(parseInt(e.key));
  } else if (key === 'o' || key === 'x') {
    answerPanelRef.value?.selectChoice(key === 'o');
  }
}
</script>

<style scoped>
.feedback-in {
  animation: feedback-in 0.3s ease-out;
}
@keyframes feedback-in {
  from { opacity: 0; transform: translateY(8px); }
  to { opacity: 1; transform: none; }
}
@media (prefers-reduced-motion: reduce) {
  .feedback-in { animation: none; }
}
</style>

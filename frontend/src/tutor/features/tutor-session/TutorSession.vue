<template>
  <div class="min-h-screen bg-[#faf6f0] dark:bg-[#1c1917] text-stone-900 dark:text-stone-100 flex flex-col p-6 md:p-8">
    <!-- Header -->
    <header class="flex items-center justify-between pb-6 border-b-2 border-stone-200 dark:border-stone-800">
      <div class="flex items-center gap-4">
        <h1 class="text-3xl font-black text-amber-700 dark:text-amber-500">
          AI 家教複習
        </h1>
        <span v-if="session" class="text-2xl font-bold bg-stone-200 dark:bg-stone-800 px-4 py-1.5 rounded-full">
          第 {{ session.index }} / {{ session.total }} 題
        </span>
      </div>

      <div class="flex items-center gap-4">
        <button
          type="button"
          class="px-5 py-3 rounded-2xl bg-amber-100 hover:bg-amber-200 text-amber-900 dark:bg-amber-950 dark:text-amber-200 font-bold text-xl cursor-pointer"
          @click="askModal = true"
        >
          ❓ 我不懂 (?)
        </button>
        <button
          type="button"
          class="px-5 py-3 rounded-2xl bg-stone-200 hover:bg-stone-300 dark:bg-stone-800 dark:hover:bg-stone-700 font-bold text-xl cursor-pointer"
          @click="end"
        >
          結束複習
        </button>
      </div>
    </header>

    <!-- Main Content (16:9 layout) -->
    <main v-if="session && session.question" class="flex-1 grid grid-cols-1 lg:grid-cols-12 gap-8 my-6">
      <!-- Left Column: QuestionCard & BookRef (7 cols) -->
      <div class="lg:col-span-7 flex flex-col gap-6">
        <QuestionCard :question="session.question" />
        <BookRefCallout :book-ref="currentBookRef" />

        <!-- Reveal explanation if revealed or explained -->
        <div
          v-if="['revealed', 'explained'].includes(session.qState) && lastReply?.reveal"
          class="bg-emerald-50 dark:bg-emerald-950/40 border-3 border-emerald-500 rounded-3xl p-6 flex flex-col gap-2"
        >
          <span class="text-xl font-bold text-emerald-800 dark:text-emerald-300">標準答案：{{ lastReply.reveal.answer }}</span>
          <p class="text-lg text-emerald-900 dark:text-emerald-200 leading-relaxed">{{ lastReply.reveal.explanation }}</p>
        </div>
      </div>

      <!-- Right Column: TutorBubble (5 cols) -->
      <div class="lg:col-span-5 flex flex-col justify-between bg-white dark:bg-stone-850 rounded-3xl p-6 shadow-md border-2 border-stone-200 dark:border-stone-800">
        <div class="flex items-center justify-between border-b-2 border-stone-100 dark:border-stone-800 pb-3 mb-2">
          <span class="text-xl font-bold text-stone-600 dark:text-stone-300">對話紀錄</span>
          <span v-if="aiThinking" class="text-amber-600 font-bold animate-pulse text-lg">
            🤔 老師想一下中...
          </span>
        </div>

        <TutorBubble :dialog="session.dialog" class="flex-1" />
      </div>
    </main>

    <!-- Bottom: AnswerPanel -->
    <footer v-if="session && session.question" class="pt-4 border-t-2 border-stone-200 dark:border-stone-800">
      <AnswerPanel
        ref="answerPanelRef"
        :question="session.question"
        :disabled="aiThinking"
        :can-next="['revealed', 'explained'].includes(session.qState)"
        @submit="submitAnswer"
        @next="next"
      />
    </footer>

    <!-- Ask Modal -->
    <div
      v-if="askModal"
      class="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4"
    >
      <div class="bg-white dark:bg-stone-800 rounded-3xl p-8 max-w-xl w-full shadow-2xl flex flex-col gap-6">
        <h3 class="text-3xl font-black">有什麼不懂的地方嗎？</h3>
        <textarea
          v-model="askText"
          class="w-full h-36 p-4 text-2xl rounded-2xl border-3 border-stone-300 dark:border-stone-600 bg-stone-50 dark:bg-stone-900 focus:border-amber-500 outline-none resize-none"
          placeholder="例如：我看不懂地圖上的圖例..."
        />
        <div class="flex items-center justify-end gap-4">
          <button
            type="button"
            class="px-6 py-3 rounded-2xl bg-stone-200 hover:bg-stone-300 dark:bg-stone-700 text-xl font-bold"
            @click="askModal = false"
          >
            取消
          </button>
          <BigButton variant="primary" @click="sendStudentMessage">
            發問 (Enter)
          </BigButton>
        </div>
      </div>
    </div>

    <!-- Self Grade Modal -->
    <div
      v-if="selfGradeModal && lastReply?.reveal"
      class="fixed inset-0 z-50 bg-black/70 flex items-center justify-center p-4"
    >
      <div class="bg-white dark:bg-stone-800 rounded-3xl p-8 max-w-xl w-full shadow-2xl flex flex-col gap-6 text-center">
        <h3 class="text-3xl font-black">自己對答案</h3>
        <div class="p-6 bg-stone-100 dark:bg-stone-900 rounded-2xl text-left">
          <div class="text-xl font-bold mb-2">標準答案：</div>
          <div class="text-3xl font-black text-amber-600 mb-4">{{ lastReply.reveal.answer }}</div>
          <div class="text-lg text-stone-600 dark:text-stone-300">{{ lastReply.reveal.explanation }}</div>
        </div>
        <p class="text-xl font-bold">對照你的手寫答案，你覺得你答對了嗎？</p>
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
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useRoute } from 'vue-router';
import { useTutorSession } from './useTutorSession';
import QuestionCard from './QuestionCard.vue';
import BookRefCallout from './BookRefCallout.vue';
import TutorBubble from './TutorBubble.vue';
import AnswerPanel from './AnswerPanel.vue';
import BigButton from '../../shared/BigButton.vue';

const route = useRoute();
const sessionId = route.params.id as string;
const answerPanelRef = ref<InstanceType<typeof AnswerPanel> | null>(null);

const {
  session,
  loading,
  aiThinking,
  selfGradeModal,
  lastReply,
  askModal,
  askText,
  load,
  submitAnswer,
  selfGrade,
  next,
  end,
  sendStudentMessage,
} = useTutorSession(sessionId);

const currentBookRef = computed(() => {
  return lastReply.value?.bookRef || (session.value?.question?.bookPage ? {
    page: session.value.question.bookPage,
    where: session.value.question.bookWhere || '',
  } : undefined);
});

onMounted(() => {
  load();
  window.addEventListener('keydown', handleGlobalKeydown);
});

onUnmounted(() => {
  window.removeEventListener('keydown', handleGlobalKeydown);
});

function handleGlobalKeydown(e: KeyboardEvent) {
  if (['input', 'textarea'].includes((e.target as HTMLElement).tagName.toLowerCase())) {
    return;
  }

  if (e.key === 'Enter') {
    e.preventDefault();
    if (['revealed', 'explained'].includes(session.value?.qState || '')) {
      next();
    } else {
      answerPanelRef.value?.submit();
    }
  } else if (e.key.toLowerCase() === 'c') {
    answerPanelRef.value?.clear();
  } else if (e.key.toLowerCase() === 'z') {
    answerPanelRef.value?.undo();
  } else if (e.key === '?') {
    askModal.value = true;
  } else if (['1', '2', '3', '4'].includes(e.key)) {
    const idx = parseInt(e.key) - 1;
    answerPanelRef.value?.selectChoice(idx);
  } else if (e.key.toUpperCase() === 'O') {
    answerPanelRef.value?.selectChoice('O');
  } else if (e.key.toUpperCase() === 'X') {
    answerPanelRef.value?.selectChoice('X');
  }
}
</script>

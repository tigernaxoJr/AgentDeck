import { ref, computed } from 'vue';
import type { ChoiceValue, SessionView, TutorReply } from '../../shared/types';
import * as api from './api';

export interface AnswerPayload {
  kind: 'choice' | 'text' | 'handwriting';
  choice?: ChoiceValue;
  text?: string;
  image_png_base64?: string;
  /** 顯示在對話紀錄中的學童作答文字（不送後端） */
  label: string;
}

export interface SessionSummary {
  answered: number;
  firstTryCorrect: number;
  weakConcepts: string[];
}

export function useTutorSession(sessionId: string) {
  const session = ref<SessionView | null>(null);
  const loading = ref(false);
  const aiThinking = ref(false);
  const selfGradeModal = ref(false);
  const lastReply = ref<TutorReply | null>(null);
  const askText = ref('');
  const summary = ref<SessionSummary | null>(null);
  const toast = ref('');
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  const canNext = computed(() => ['revealed', 'explained'].includes(session.value?.qState || ''));
  const isLast = computed(() => !!session.value && session.value.index >= session.value.total);

  /** friendly：AI 相關錯誤訊息是給大人看的，孩子只看到簡單說明 */
  function showError(e: unknown, fallback: string, friendly = false) {
    console.error(e);
    toast.value = friendly ? fallback : (e as Error)?.message || fallback;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast.value = ''), 4000);
  }

  async function load() {
    loading.value = true;
    try {
      session.value = await api.fetchSession(sessionId);
    } catch (e) {
      showError(e, '載入題目失敗');
    } finally {
      loading.value = false;
    }
  }

  async function submitAnswer({ label, ...payload }: AnswerPayload) {
    if (!session.value?.question || aiThinking.value) return;

    session.value.dialog.push({ role: 'student', content: label });
    aiThinking.value = true;
    try {
      const reply = await api.postAnswer(sessionId, {
        question_id: session.value.question.id,
        ...payload,
      });

      lastReply.value = reply;
      session.value.qState = reply.qState;
      session.value.dialog.push({
        role: 'tutor',
        content: reply.say,
        bookRef: reply.bookRef,
      });

      if (reply.selfGradeAvailable) {
        selfGradeModal.value = true;
      }
    } catch (e) {
      // 沒送成功就把剛剛的作答泡泡收回，讓孩子重送
      const last = session.value?.dialog.at(-1);
      if (last?.role === 'student' && last.content === label) session.value!.dialog.pop();
      if (!(e as Error).message?.includes('Conflict')) {
        showError(e, '老師剛剛沒有回應，請再按一次「送出答案」', true);
      }
    } finally {
      aiThinking.value = false;
    }
  }

  async function selfGrade(correct: boolean) {
    if (!session.value?.question) return;
    try {
      const reply = await api.postSelfGrade(sessionId, {
        question_id: session.value.question.id,
        correct,
      });
      selfGradeModal.value = false;
      lastReply.value = reply;
      session.value.qState = reply.qState;
      session.value.dialog.push({ role: 'tutor', content: reply.say });
    } catch (e) {
      showError(e, '自評失敗');
    }
  }

  async function next() {
    if (loading.value || !canNext.value) return;
    if (isLast.value) return finish();
    loading.value = true;
    try {
      session.value = await api.postNext(sessionId);
      lastReply.value = null;
    } catch (e) {
      showError(e, '切換下一題失敗');
    } finally {
      loading.value = false;
    }
  }

  async function finish() {
    if (summary.value) return;
    try {
      summary.value = await api.postEnd(sessionId);
    } catch (e) {
      showError(e, '結束複習失敗');
    }
  }

  async function end() {
    if (!confirm('確定要結束這次複習嗎？')) return;
    await finish();
  }

  async function sendStudentMessage() {
    if (!askText.value.trim() || !session.value?.question || aiThinking.value) return;
    const txt = askText.value.trim();
    askText.value = '';

    session.value.dialog.push({ role: 'student', content: txt });

    aiThinking.value = true;
    try {
      const reply = await api.postMessage(sessionId, {
        question_id: session.value.question.id,
        text: txt,
      });
      session.value.dialog.push({
        role: 'tutor',
        content: reply.say,
        bookRef: reply.bookRef,
      });
    } catch (e) {
      // 把問題放回輸入框，方便重問
      if (session.value?.dialog.at(-1)?.content === txt) session.value.dialog.pop();
      askText.value = txt;
      showError(e, '老師剛剛沒有回應，請再問一次', true);
    } finally {
      aiThinking.value = false;
    }
  }

  return {
    session,
    loading,
    aiThinking,
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
  };
}

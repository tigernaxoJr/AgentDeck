import { ref, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import type { SessionView, TutorReply } from '../../shared/types';
import * as api from './api';

export function useTutorSession(sessionId: string) {
  const router = useRouter();
  const session = ref<SessionView | null>(null);
  const loading = ref(false);
  const aiThinking = ref(false);
  const selfGradeModal = ref(false);
  const lastReply = ref<TutorReply | null>(null);
  const askModal = ref(false);
  const askText = ref('');

  async function load() {
    loading.value = true;
    try {
      session.value = await api.fetchSession(sessionId);
    } catch (e: any) {
      alert(e.message || '載入題目失敗');
    } finally {
      loading.value = false;
    }
  }

  async function submitAnswer(payload: {
    kind: 'choice' | 'text' | 'handwriting';
    choice?: string | number;
    text?: string;
    image_png_base64?: string;
  }) {
    if (!session.value?.question || aiThinking.value) return;

    aiThinking.value = true;
    try {
      const reply = await api.postAnswer(sessionId, {
        question_id: session.value.question.id,
        ...payload,
      });

      lastReply.value = reply;
      session.value.qState = reply.qState;

      // Add to dialog list
      session.value.dialog.push({
        role: 'tutor',
        content: reply.say,
        bookRef: reply.bookRef,
      });

      if (reply.selfGradeAvailable) {
        selfGradeModal.value = true;
      }
    } catch (e: any) {
      if (!e.message?.includes('Conflict')) {
        alert(e.message || '送出作答失敗');
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
      session.value.dialog.push({
        role: 'tutor',
        content: reply.say,
      });
    } catch (e: any) {
      alert(e.message || '自評失敗');
    }
  }

  async function next() {
    if (loading.value) return;
    loading.value = true;
    try {
      session.value = await api.postNext(sessionId);
      lastReply.value = null;
    } catch (e: any) {
      alert(e.message || '切換下一題失敗');
    } finally {
      loading.value = false;
    }
  }

  async function end() {
    if (!confirm('確定要結束這次複習嗎？')) return;
    try {
      await api.postEnd(sessionId);
      router.push('/tutor');
    } catch (e: any) {
      alert(e.message || '結束複習失敗');
    }
  }

  async function sendStudentMessage() {
    if (!askText.value.trim() || !session.value?.question) return;
    const txt = askText.value.trim();
    askText.value = '';
    askModal.value = false;

    session.value.dialog.push({
      role: 'student',
      content: txt,
    });

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
    } catch (e: any) {
      alert(e.message || '問老師失敗');
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
    askModal,
    askText,
    load,
    submitAnswer,
    selfGrade,
    next,
    end,
    sendStudentMessage,
  };
}

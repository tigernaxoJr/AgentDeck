import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SessionView, TutorReply } from '../../shared/types';
import * as api from './api';
import { useTutorSession } from './useTutorSession';

vi.mock('./api');
const mocked = vi.mocked(api);

function sessionView(): SessionView {
  return {
    sessionId: 's1',
    mode: 'progress',
    index: 1,
    total: 2,
    qState: 'asking',
    question: { id: 'q1', qtype: 'single_choice', inputMode: 'choice', section: '', stem: '題目', options: ['東', '西'] },
    dialog: [],
  };
}

function reply(over: Partial<TutorReply> = {}): TutorReply {
  return { verdict: 'incorrect', say: '差一點點！', action: 'hint', qState: 'hinting', canNext: false, ...over };
}

/** 可以從外部決定何時 resolve 的 promise，用來模擬慢回應 */
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

async function loaded() {
  mocked.fetchSession.mockResolvedValue(sessionView());
  const s = useTutorSession('s1');
  await s.load();
  return s;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

describe('useTutorSession', () => {
  it('shows the verdict first, then appends the AI hint', async () => {
    const s = await loaded();
    mocked.postAnswer.mockResolvedValue(reply({ hintPending: true }));
    const hint = deferred<TutorReply>();
    mocked.postHint.mockReturnValue(hint.promise);

    await s.submitAnswer({ kind: 'choice', choice: 1, label: '東' });
    expect(s.session.value?.qState).toBe('hinting');
    expect(s.hintLoading.value).toBe(true);
    expect(s.session.value?.dialog.map((d) => d.role)).toEqual(['student']);

    hint.resolve(reply({ say: '想想地圖的上方是哪裡？', bookRef: { page: 12, where: '上方' } }));
    await vi.waitFor(() => expect(s.hintLoading.value).toBe(false));
    expect(s.session.value?.dialog.at(-1)).toEqual({
      role: 'tutor',
      content: '想想地圖的上方是哪裡？',
      bookRef: { page: 12, where: '上方' },
    });
  });

  it('falls back to the canned hint when /hint fails', async () => {
    const s = await loaded();
    mocked.postAnswer.mockResolvedValue(reply({ hintPending: true, say: '備用提示' }));
    mocked.postHint.mockRejectedValue(new Error('boom'));

    await s.submitAnswer({ kind: 'choice', choice: 1, label: '東' });
    await vi.waitFor(() => expect(s.hintLoading.value).toBe(false));
    expect(s.session.value?.dialog.at(-1)?.content).toBe('備用提示');
  });

  it('drops a stale hint once the child has answered again', async () => {
    const s = await loaded();
    const hint = deferred<TutorReply>();
    mocked.postHint.mockReturnValue(hint.promise);
    mocked.postAnswer
      .mockResolvedValueOnce(reply({ hintPending: true }))
      .mockResolvedValueOnce(reply({ verdict: 'correct', say: '答對了！', action: 'praise', qState: 'revealed', canNext: true }));

    await s.submitAnswer({ kind: 'choice', choice: 1, label: '東' });
    await s.submitAnswer({ kind: 'choice', choice: 2, label: '西' });
    hint.resolve(reply({ say: '過時的提示' }));
    await hint.promise;
    await Promise.resolve();

    const contents = s.session.value?.dialog.map((d) => d.content);
    expect(contents).toEqual(['東', '西', '答對了！']);
    expect(s.canNext.value).toBe(true);
  });

  it('removes the student bubble when submitting fails', async () => {
    const s = await loaded();
    mocked.postAnswer.mockRejectedValue(new Error('network'));

    await s.submitAnswer({ kind: 'choice', choice: 1, label: '東' });
    expect(s.session.value?.dialog).toEqual([]);
    expect(s.toast.value).not.toBe('');
    expect(s.aiThinking.value).toBe(false);
  });

  it('opens the self-grade modal when AI grading is unavailable', async () => {
    const s = await loaded();
    mocked.postAnswer.mockResolvedValue(reply({ verdict: 'none', action: 'self_grade', qState: 'asking', selfGradeAvailable: true }));

    await s.submitAnswer({ kind: 'handwriting', image_png_base64: 'x', label: '（手寫）' });
    expect(s.selfGradeModal.value).toBe(true);
  });
});

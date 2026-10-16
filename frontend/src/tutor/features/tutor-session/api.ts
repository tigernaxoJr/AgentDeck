import { tutorFetch } from '../../shared/http';
import type { ChoiceValue, SessionView, TutorReply } from '../../shared/types';

export async function fetchSession(id: string): Promise<SessionView> {
  return tutorFetch<SessionView>(`/api/tutor/sessions/${id}`);
}

export async function postAnswer(id: string, payload: {
  question_id: string;
  kind: 'choice' | 'text' | 'handwriting';
  choice?: ChoiceValue;
  text?: string;
  image_png_base64?: string;
}): Promise<TutorReply> {
  return tutorFetch<TutorReply>(`/api/tutor/sessions/${id}/answer`, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function postSelfGrade(id: string, payload: {
  question_id: string;
  correct: boolean;
}): Promise<TutorReply> {
  return tutorFetch<TutorReply>(`/api/tutor/sessions/${id}/self_grade`, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function postHint(id: string, payload: {
  question_id: string;
  action: string;
}): Promise<TutorReply> {
  return tutorFetch<TutorReply>(`/api/tutor/sessions/${id}/hint`, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function postMessage(id: string, payload: {
  question_id: string;
  text: string;
}): Promise<TutorReply> {
  return tutorFetch<TutorReply>(`/api/tutor/sessions/${id}/message`, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function postNext(id: string): Promise<SessionView> {
  return tutorFetch<SessionView>(`/api/tutor/sessions/${id}/next`, {
    method: 'POST',
  });
}

export async function postEnd(id: string): Promise<{
  answered: number;
  firstTryCorrect: number;
  weakConcepts: string[];
}> {
  return tutorFetch(`/api/tutor/sessions/${id}/end`, {
    method: 'POST',
  });
}

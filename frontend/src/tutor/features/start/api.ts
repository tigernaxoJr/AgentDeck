import { tutorFetch } from '../../shared/http';
import type { StudentItem, SubjectCatalogItem, SessionView } from '../../shared/types';

export async function fetchStudents(): Promise<StudentItem[]> {
  return tutorFetch<StudentItem[]>('/api/tutor/students');
}

export async function createStudent(name: string, grade: number): Promise<StudentItem> {
  return tutorFetch<StudentItem>('/api/tutor/students', {
    method: 'POST',
    body: JSON.stringify({ name, grade }),
  });
}

export async function fetchSubjects(studentId?: number): Promise<SubjectCatalogItem[]> {
  const q = studentId ? `?student_id=${studentId}` : '';
  return tutorFetch<SubjectCatalogItem[]>(`/api/tutor/subjects${q}`);
}

export async function startSession(payload: {
  student_id: number;
  mode: string;
  subject_id: string;
  unit_id?: string;
  limit?: number;
}): Promise<SessionView> {
  return tutorFetch<SessionView>('/api/tutor/sessions', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

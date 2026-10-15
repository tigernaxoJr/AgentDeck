import { tutorFetch } from '../../shared/http';

export interface StudentReportView {
  weakConcepts: Array<{
    tag: string;
    score: number;
    wrongCount: number;
    lastNote?: string;
  }>;
  dueCount: number;
  recentWrong: Array<{
    question: any;
    attempts: Array<{
      verdict: string;
      inputImageUrl?: string;
      inputText?: string;
      recognizedText?: string;
      gradedBy: string;
      createdAt: string;
    }>;
  }>;
}

export async function fetchStudentReport(studentId: number): Promise<StudentReportView> {
  return tutorFetch<StudentReportView>(`/api/tutor/students/${studentId}/report`);
}

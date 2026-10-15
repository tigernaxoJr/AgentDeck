import { tutorFetch } from '../../shared/http';

export async function fetchBackends(): Promise<{
  available: Array<{
    id: string;
    version?: string;
    healthy: boolean;
    capabilities: any;
    model?: string;
  }>;
  active: string[];
  activeVision: string[];
}> {
  return tutorFetch('/api/tutor/admin/ai/backends');
}

export async function updateBackends(payload: {
  active: string[];
  activeVision?: string[];
}): Promise<any> {
  return tutorFetch('/api/tutor/admin/ai/backends', {
    method: 'PUT',
    body: JSON.stringify(payload),
  });
}

export async function testBackend(backendId: string, withImage: boolean = false): Promise<{
  ok: boolean;
  durationMs: number;
  reply?: string;
  error?: string;
}> {
  return tutorFetch('/api/tutor/admin/ai/test', {
    method: 'POST',
    body: JSON.stringify({ backend_id: backendId, with_image: withImage }),
  });
}

export async function triggerEnrich(subjectId: string): Promise<any> {
  return tutorFetch('/api/tutor/admin/enrich', {
    method: 'POST',
    body: JSON.stringify({ subject_id: subjectId }),
  });
}

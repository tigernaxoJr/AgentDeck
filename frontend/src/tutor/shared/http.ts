export async function tutorFetch<T>(url: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers || {});
  headers.set('Content-Type', 'application/json');

  const token = localStorage.getItem('antigravity_api_key');
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }

  const res = await fetch(url, { ...options, headers });
  if (!res.ok) {
    let errBody = { message: res.statusText };
    try {
      errBody = await res.json();
    } catch (_) {}
    throw new Error(errBody.message || `HTTP error ${res.status}`);
  }

  return res.json() as Promise<T>;
}

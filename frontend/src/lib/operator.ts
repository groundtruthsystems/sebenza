// The operator credential (FR-10). Operator-only routes refuse the shared
// control token, so the dashboard asks the daemon for its own operator token.
// The daemon issues it only to a same-origin request from this page; it lives
// in this module's memory, never in a URL, storage or `window`.

let cached: { token: string; expiresAt: number } | null = null;

/** The operator token, fetched once and re-fetched after it expires. */
export async function operatorToken(): Promise<string | null> {
  const now = Math.floor(Date.now() / 1000);
  if (cached && cached.expiresAt > now + 30) return cached.token;
  try {
    const res = await fetch("/api/operator/session", {
      method: "POST",
      credentials: "same-origin",
    });
    if (!res.ok) return null;
    const data = (await res.json()) as { token?: string; expiresAt?: number };
    if (!data.token) return null;
    cached = { token: data.token, expiresAt: data.expiresAt ?? now + 3600 };
    return cached.token;
  } catch {
    return null;
  }
}

/** Forget the token (tests, or after a 401 from a restarted daemon). */
export function clearOperatorToken(): void {
  cached = null;
}

async function operatorFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const token = await operatorToken();
  const res = await fetch(path, {
    ...init,
    credentials: "same-origin",
    headers: {
      ...(init.headers as Record<string, string> | undefined),
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
  });
  // A restarted daemon voids the token: fetch a new one and retry once.
  if (res.status === 401 && token) {
    clearOperatorToken();
    const fresh = await operatorToken();
    if (fresh) {
      return fetch(path, {
        ...init,
        credentials: "same-origin",
        headers: {
          ...(init.headers as Record<string, string> | undefined),
          authorization: `Bearer ${fresh}`,
        },
      });
    }
  }
  return res;
}

export interface PendingLogin {
  loginId: string;
  code: string;
  client: string;
  createdAt: number;
}

/** CLI sign-ins waiting for the operator. */
export async function fetchPendingLogins(): Promise<PendingLogin[]> {
  const res = await operatorFetch("/api/operator/logins");
  if (!res.ok) return [];
  const data = (await res.json()) as { logins?: PendingLogin[] };
  return data.logins ?? [];
}

export async function decideLogin(loginId: string, decision: "confirm" | "deny"): Promise<void> {
  const res = await operatorFetch(
    `/api/operator/logins/${encodeURIComponent(loginId)}/${decision}`,
    { method: "POST" },
  );
  if (!res.ok) {
    const data = (await res.json().catch(() => ({}))) as { error?: string };
    throw new Error(data.error ?? `HTTP ${res.status}`);
  }
}

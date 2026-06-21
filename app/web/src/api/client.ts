import type {
  AiSettingsUpdate,
  AiSummary,
  ConfigSummary,
  HealthResponse,
  SessionDetail,
  SessionRef,
  SessionsResponse,
  ShareLink
} from "./types";

const tokenKey = "coca-web-token";

export function readToken(): string {
  const params = new URLSearchParams(window.location.search);
  const token = params.get("token");
  if (token) {
    window.localStorage.setItem(tokenKey, token);
    params.delete("token");
    const next = `${window.location.pathname}${params.toString() ? `?${params}` : ""}${window.location.hash}`;
    window.history.replaceState(null, "", next);
    return token;
  }
  return window.localStorage.getItem(tokenKey) ?? "";
}

export function saveToken(token: string) {
  window.localStorage.setItem(tokenKey, token);
}

export function clearToken() {
  window.localStorage.removeItem(tokenKey);
}

export class ApiClient {
  constructor(private token: string) {}

  health() {
    return this.get<HealthResponse>("/api/v1/health");
  }

  sessions() {
    return this.get<SessionsResponse>("/api/v1/sessions");
  }

  session(ref: SessionRef) {
    const query = new URLSearchParams(ref);
    return this.get<SessionDetail>(`/api/v1/session?${query}`);
  }

  configSummary() {
    return this.get<ConfigSummary>("/api/v1/config/summary");
  }

  updateAiConfig(update: AiSettingsUpdate) {
    return this.put<AiSummary>("/api/v1/config/ai", update);
  }

  shareSession(session: SessionRef) {
    return this.post<ShareLink>("/api/v1/share-session", { session });
  }

  private async get<T>(path: string): Promise<T> {
    const response = await fetch(path, {
      headers: this.headers()
    });
    return decode<T>(response);
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const response = await fetch(path, {
      method: "POST",
      headers: {
        ...this.headers(),
        "Content-Type": "application/json"
      },
      body: JSON.stringify(body)
    });
    return decode<T>(response);
  }

  private async put<T>(path: string, body: unknown): Promise<T> {
    const response = await fetch(path, {
      method: "PUT",
      headers: {
        ...this.headers(),
        "Content-Type": "application/json"
      },
      body: JSON.stringify(body)
    });
    return decode<T>(response);
  }

  private headers() {
    return {
      Authorization: `Bearer ${this.token}`
    };
  }
}

async function decode<T>(response: Response): Promise<T> {
  if (!response.ok) {
    const body = await response.text();
    throw new Error(body || `HTTP ${response.status}`);
  }
  return response.json() as Promise<T>;
}

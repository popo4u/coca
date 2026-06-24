import type {
  AccountDevicesResponse,
  AccountMe,
  AccountTokenCreateResponse,
  AccountTokensResponse,
  AuthCapabilities,
  AuthLoginRequest,
  AuthSessionResponse,
  AuthSignupRequest,
  AiSettingsUpdate,
  AiSummary,
  ConfigSummary,
  HealthResponse,
  PasswordUpdate,
  ProfileUpdate,
  SessionDetail,
  SessionRef,
  SessionsResponse,
  ShareLink,
  StructuredError,
  TerminalClientFrame,
  TerminalSessionsResponse
} from "./types";

const tokenKey = "coca-web-token";
const terminalTokenKey = "coca-web-terminal-token";

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

export function readTerminalToken(): string {
  const params = new URLSearchParams(window.location.search);
  const token = params.get("terminal_token");
  if (token) {
    window.localStorage.setItem(terminalTokenKey, token);
    params.delete("terminal_token");
    const next = `${window.location.pathname}${params.toString() ? `?${params}` : ""}${window.location.hash}`;
    window.history.replaceState(null, "", next);
    return token;
  }
  return window.localStorage.getItem(terminalTokenKey) ?? "";
}

export function saveTerminalToken(token: string) {
  window.localStorage.setItem(terminalTokenKey, token);
}

export function clearTerminalToken() {
  window.localStorage.removeItem(terminalTokenKey);
}

export function openTerminalSocket(readToken: string, terminalToken: string): WebSocket {
  const params = new URLSearchParams({
    token: readToken,
    terminal_token: terminalToken
  });
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return new WebSocket(`${protocol}//${window.location.host}/api/v1/terminal/ws?${params}`);
}

export function sendTerminalFrame(socket: WebSocket, frame: TerminalClientFrame) {
  socket.send(JSON.stringify(frame));
}

export class ApiClient {
  constructor(private token: string) {}

  health() {
    return this.get<HealthResponse>("/api/v1/health");
  }

  authCapabilities() {
    return this.get<AuthCapabilities>("/api/v1/auth/capabilities");
  }

  login(request: AuthLoginRequest) {
    return this.post<AuthSessionResponse>("/api/v1/auth/login", request);
  }

  signup(request: AuthSignupRequest) {
    return this.post<AuthSessionResponse>("/api/v1/auth/signup", request);
  }

  logout() {
    return this.post<Record<string, never>>("/api/v1/auth/logout", {});
  }

  accountMe() {
    return this.get<AccountMe>("/api/v1/account/me");
  }

  updateProfile(update: ProfileUpdate) {
    return this.patch<AccountMe["user"]>("/api/v1/account/profile", update);
  }

  updatePassword(update: PasswordUpdate) {
    return this.post<Record<string, never>>("/api/v1/account/password", update);
  }

  accountDevices() {
    return this.get<AccountDevicesResponse>("/api/v1/account/devices");
  }

  revokeDevice(deviceId: string) {
    return this.post<Record<string, never>>("/api/v1/account/devices/revoke", { session_id: deviceId });
  }

  accountTokens() {
    return this.get<AccountTokensResponse>("/api/v1/account/tokens");
  }

  createAccountToken(name: string) {
    return this.post<AccountTokenCreateResponse>("/api/v1/account/tokens", { name });
  }

  revokeAccountToken(tokenId: string) {
    return this.post<Record<string, never>>("/api/v1/account/tokens/revoke", { token_id: tokenId });
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

  terminalSessions(terminalToken: string) {
    return this.get<TerminalSessionsResponse>("/api/v1/terminal/sessions", {
      "X-Coca-Terminal-Token": terminalToken
    });
  }

  private async get<T>(path: string, extraHeaders?: Record<string, string>): Promise<T> {
    const response = await fetch(path, {
      headers: {
        ...this.headers(),
        ...extraHeaders
      }
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

  private async patch<T>(path: string, body: unknown): Promise<T> {
    const response = await fetch(path, {
      method: "PATCH",
      headers: {
        ...this.headers(),
        "Content-Type": "application/json"
      },
      body: JSON.stringify(body)
    });
    return decode<T>(response);
  }

  private headers(): Record<string, string> {
    return this.token ? { Authorization: `Bearer ${this.token}` } : {};
  }
}

async function decode<T>(response: Response): Promise<T> {
  if (!response.ok) {
    const body = await response.text();
    throw apiErrorFromBody(body, response.status);
  }
  return response.json() as Promise<T>;
}

export class ApiError extends Error {
  constructor(
    message: string,
    public code: string,
    public action: string | null = null,
    public detail: string | null = null
  ) {
    super(message);
    this.name = "ApiError";
  }
}

function apiErrorFromBody(body: string, status: number): ApiError {
  if (body.trim().startsWith("{")) {
    try {
      const parsed = JSON.parse(body) as Partial<StructuredError>;
      if (typeof parsed.message === "string" || typeof parsed.code === "string") {
        return new ApiError(
          parsed.message || `HTTP ${status}`,
          parsed.code || `http_${status}`,
          typeof parsed.action === "string" ? parsed.action : null,
          typeof parsed.detail === "string" ? parsed.detail : null
        );
      }
    } catch {
      // Fall through to text handling.
    }
  }
  return new ApiError(body || `HTTP ${status}`, `http_${status}`);
}

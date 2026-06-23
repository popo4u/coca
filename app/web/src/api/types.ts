export type SessionSummary = {
  origin: string;
  provider: string;
  id: string;
  title: string;
  cwd: string;
  updated_at_ms: number | null;
  updated_label: string;
  model: string | null;
  message_count: number;
  first_user_message: string | null;
  terminal: TerminalCapability;
};

export type TerminalCapability = {
  enabled: boolean;
  can_resume: boolean;
  can_fork: boolean;
  unavailable_code: string | null;
  unavailable_message: string | null;
};

export type CatalogCounts = {
  total: number;
  by_provider: Record<string, number>;
  by_origin: Record<string, number>;
};

export type SessionsResponse = {
  sessions: SessionSummary[];
  warnings: string[];
  counts: CatalogCounts;
};

export type ChatMessage = {
  role: string;
  display_role: string;
  text: string;
  timestamp_ms: number | null;
  timestamp_label: string;
};

export type SessionDetail = {
  summary: SessionSummary;
  transcript: ChatMessage[];
};

export type ConfigSummary = {
  service: string;
  version: string;
  bind: string;
  gateway_bind: string;
  ai: AiSummary;
  share: {
    base_url: string;
    token_configured: boolean;
  };
  terminal: {
    enabled: boolean;
    token_configured: boolean;
    daemon_available: boolean;
    terminal_socket_available: boolean;
    unavailable_code: string | null;
    unavailable_message: string | null;
  };
  remotes: Array<{
    name: string;
    base_url: string;
    enabled: boolean;
    visible: boolean;
    token_configured: boolean;
    terminal_token_configured: boolean;
    terminal_ready: boolean;
    terminal_unavailable_code: string | null;
    terminal_unavailable_message: string | null;
    session_count: number;
  }>;
  launch_defaults: {
    resume: { use_current_dir: boolean; yolo: boolean };
    fork: { use_current_dir: boolean; yolo: boolean };
  };
  counts: CatalogCounts;
  warnings: string[];
};

export type AiSummary = {
  base_url: string;
  model: string;
  enabled: boolean;
  provider: string;
  api_key_env: string;
  api_key_configured: boolean;
  key_source: string;
};

export type AiSettingsUpdate = {
  base_url?: string;
  model?: string;
  enabled?: boolean;
  provider?: string;
  api_key_env?: string;
  api_key?: string;
  clear_api_key?: boolean;
};

export type StreamInfo = {
  protocol: string;
  client_events: string[];
  server_events: string[];
};

export type HealthResponse = {
  ok: boolean;
  service: string;
  version: string;
  stream: StreamInfo;
};

export type SessionRef = {
  origin: string;
  provider: string;
  id: string;
};

export type ShareLink = {
  url: string;
};

export type TerminalId = string;
export type TerminalSeq = number;

export type TerminalSize = {
  cols: number;
  rows: number;
};

export type TerminalMode = "Resume" | "Fork";
export type TerminalState = "Starting" | "Running" | "Detached" | "Exited";

export type TerminalExitInfo = {
  code: number | null;
  signal: string | null;
};

export type TerminalSessionSummary = {
  terminal_id: TerminalId;
  session: SessionRef;
  mode: TerminalMode;
  state: TerminalState;
  attached_clients: number;
  active_writer: string | null;
  last_seq: TerminalSeq;
  size: TerminalSize;
  exit: TerminalExitInfo | null;
};

export type TerminalSessionsResponse = {
  terminals: TerminalSessionSummary[];
};

export type TerminalClientFrame =
  | { event: "terminal.open"; payload: { session: SessionRef; mode: TerminalMode; size: TerminalSize } }
  | { event: "terminal.attach"; payload: { terminal_id: TerminalId; since_seq: TerminalSeq | null; size: TerminalSize } }
  | { event: "terminal.input"; payload: { terminal_id: TerminalId; data_b64: string } }
  | { event: "terminal.resize"; payload: { terminal_id: TerminalId; size: TerminalSize } }
  | { event: "terminal.detach"; payload: { terminal_id: TerminalId } }
  | { event: "terminal.close"; payload: { terminal_id: TerminalId; kill: boolean } };

export type TerminalServerFrame =
  | { event: "terminal.opened"; payload: { terminal: TerminalSessionSummary } }
  | { event: "terminal.output"; payload: { terminal_id: TerminalId; seq: TerminalSeq; data_b64: string } }
  | { event: "terminal.exit"; payload: { terminal_id: TerminalId; exit: TerminalExitInfo } }
  | { event: "terminal.error"; payload: StructuredError & { request_id: string | null; terminal_id: TerminalId | null } };

export type StructuredError = {
  code: string;
  message: string;
  action?: string | null;
  detail?: string | null;
};

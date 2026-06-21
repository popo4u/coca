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
  core_bind: string;
  ai: AiSummary;
  share: {
    base_url: string;
    token_configured: boolean;
  };
  remotes: Array<{
    name: string;
    base_url: string;
    enabled: boolean;
    visible: boolean;
    token_configured: boolean;
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

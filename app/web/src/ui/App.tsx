import { FormEvent, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import {
  Activity,
  Bot,
  Cable,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  Clock3,
  Code2,
  Cpu,
  Database,
  Eye,
  FileText,
  Folder,
  KeyRound,
  LogOut,
  Moon,
  Search,
  Share2,
  Sun,
  TerminalSquare,
  UserRound
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ApiClient, clearToken, readToken, saveToken } from "../api/client";
import type { ConfigSummary, HealthResponse, SessionDetail, SessionRef, SessionSummary, SessionsResponse } from "../api/types";

type View =
  | { name: "sessions" }
  | { name: "config" }
  | { name: "detail"; ref: SessionRef };

type LoadState<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; data: T }
  | { status: "error"; error: string };

type Theme = "dark" | "light";
type TranscriptRole = "user" | "assistant" | "context" | "event";
type MessageMode = "preview" | "raw";

export function App() {
  const [token, setToken] = useState(readToken);
  const [view, setView] = useState<View>(() => routeFromHash());
  const [theme, setTheme] = useState<Theme>(() => readTheme());
  const client = useMemo(() => new ApiClient(token), [token]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem("coca-web-theme", theme);
  }, [theme]);

  useEffect(() => {
    const onHash = () => setView(routeFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  if (!token) {
    return (
      <TokenGate onSubmit={(value) => {
        saveToken(value);
        setToken(value);
      }} />
    );
  }

  return (
    <Shell
      view={view}
      onNavigate={setView}
      onLogout={() => {
        clearToken();
        setToken("");
      }}
    >
      {view.name === "sessions" && <SessionsView client={client} onOpen={openDetail} />}
      {view.name === "config" && <ConfigView client={client} theme={theme} onThemeChange={setTheme} />}
      {view.name === "detail" && <DetailView client={client} reference={view.ref} />}
    </Shell>
  );

  function openDetail(session: SessionSummary) {
    const ref = { origin: session.origin, provider: session.provider, id: session.id };
    window.location.hash = `session/${encodePart(ref.origin)}/${encodePart(ref.provider)}/${encodePart(ref.id)}`;
    setView({ name: "detail", ref });
  }
}

function TokenGate({ onSubmit }: { onSubmit: (token: string) => void }) {
  const [value, setValue] = useState("");
  return (
    <main className="gate">
      <section className="gate-panel">
        <div className="mark"><TerminalSquare size={28} /></div>
        <h1>coca</h1>
        <form onSubmit={(event: FormEvent) => {
          event.preventDefault();
          const token = value.trim();
          if (token) onSubmit(token);
        }}>
          <label>
            <span>Access token</span>
            <input value={value} onChange={(event) => setValue(event.target.value)} autoFocus />
          </label>
          <button type="submit"><KeyRound size={16} />Enter</button>
        </form>
      </section>
    </main>
  );
}

function Shell({ children, view, onNavigate, onLogout }: {
  children: ReactNode;
  view: View;
  onNavigate: (view: View) => void;
  onLogout: () => void;
}) {
  return (
    <main className="shell">
      <aside className="rail">
        <a className="brand" href="#/" onClick={() => onNavigate({ name: "sessions" })}>
          <TerminalSquare size={22} />
          <span>coca</span>
        </a>
        <nav>
          <a className={view.name === "sessions" || view.name === "detail" ? "active" : ""} href="#/">Sessions</a>
          <a className={view.name === "config" ? "active" : ""} href="#/config">Config</a>
        </nav>
        <button className="icon-line" onClick={onLogout}><LogOut size={16} />Sign out</button>
      </aside>
      <section className="workspace">{children}</section>
    </main>
  );
}

function SessionsView({ client, onOpen }: { client: ApiClient; onOpen: (session: SessionSummary) => void }) {
  const [state, setState] = useState<LoadState<SessionsResponse>>({ status: "loading" });
  const [query, setQuery] = useState("");

  useEffect(() => {
    setState({ status: "loading" });
    client.sessions().then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Sessions" />;
  if (state.status === "error") return <ErrorPanel title="Sessions" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Sessions" />;

  const data = state.data;
  const sessions = data.sessions.filter((session: SessionSummary) => {
    const haystack = `${session.origin} ${session.provider} ${session.title} ${session.cwd} ${session.model ?? ""}`.toLowerCase();
    return haystack.includes(query.trim().toLowerCase());
  });

  return (
    <div className="stack">
      <header className="page-head">
        <div>
          <p>catalog</p>
          <h1>Agent sessions</h1>
        </div>
        <div className="metric-strip">
          <Metric icon={<Database size={16} />} label="total" value={data.counts.total} />
          <Metric icon={<Cpu size={16} />} label="providers" value={Object.keys(data.counts.by_provider).length} />
          <Metric icon={<Cable size={16} />} label="origins" value={Object.keys(data.counts.by_origin).length} />
        </div>
      </header>
      {data.warnings.length > 0 && <Warnings warnings={data.warnings} />}
      <label className="search">
        <Search size={16} />
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search sessions" />
      </label>
      <div className="session-list">
        {sessions.map((session: SessionSummary) => <SessionRow key={`${session.origin}:${session.provider}:${session.id}`} session={session} onClick={() => onOpen(session)} />)}
      </div>
    </div>
  );
}

function SessionRow({ session, onClick }: { session: SessionSummary; onClick: () => void }) {
  return (
    <button className="session-row" onClick={onClick}>
      <span className="row-top"><strong>{session.provider}</strong><em>{session.origin}</em><time>{session.updated_label}</time></span>
      <span className="row-title">{session.title}</span>
      <span className="row-meta"><code>{session.cwd}</code><span>{session.model ?? "-"}</span><span>{session.message_count} messages</span></span>
    </button>
  );
}

function DetailView({ client, reference }: { client: ApiClient; reference: SessionRef }) {
  const [state, setState] = useState<LoadState<SessionDetail>>({ status: "loading" });
  const [share, setShare] = useState("");

  useEffect(() => {
    setState({ status: "loading" });
    client.session(reference).then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client, reference.origin, reference.provider, reference.id]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Session" />;
  if (state.status === "error") return <ErrorPanel title="Session" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Session" />;

  const { summary, transcript } = state.data;
  const firstPrompt = summary.first_user_message?.trim() || "";
  const transcriptMessages = firstPrompt && transcript[0] && isFirstPromptDuplicate(firstPrompt, transcript[0]) ? transcript.slice(1) : transcript;
  return (
    <div className="stack detail-stack">
      <header className="session-title-panel">
        <div className="detail-title-block">
          <p>session detail</p>
          <h1 title={summary.title}>{summary.title}</h1>
        </div>
        <button className="icon-line" onClick={() => client.shareSession(reference).then((link) => setShare(link.url)).catch((error: Error) => setShare(error.message))}><Share2 size={16} />Share</button>
      </header>
      {share && <div className="notice"><code>{share}</code></div>}
      <section className="session-meta-panel" aria-label="Session metadata">
        <MetaTile icon={<Cable size={16} />} label="origin" value={summary.origin} />
        <MetaTile icon={<TerminalSquare size={16} />} label="provider" value={summary.provider} />
        <MetaTile icon={<Folder size={16} />} label="cwd" value={summary.cwd} />
        <MetaTile icon={<Cpu size={16} />} label="model" value={summary.model ?? "-"} />
        <MetaTile icon={<FileText size={16} />} label="messages" value={`${summary.message_count}`} />
        <MetaTile icon={<Clock3 size={16} />} label="updated" value={summary.updated_label} />
      </section>
      <section className="transcript-panel" aria-label="Transcript">
        <header className="section-head">
          <p>transcript</p>
          <h2>Conversation timeline</h2>
        </header>
        <div className="transcript">
          {!firstPrompt && transcriptMessages.length === 0 && <p className="muted">No transcript text was reconstructed.</p>}
          {firstPrompt && <FirstPromptItem prompt={firstPrompt} />}
          {transcriptMessages.map((message, index: number) => <Message key={`${message.timestamp_ms ?? index}:${index}`} message={message} index={index + (firstPrompt ? 1 : 0)} />)}
        </div>
      </section>
    </div>
  );
}

function MetaTile({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div className="meta-tile">
      <span className="meta-icon" aria-hidden="true">{icon}</span>
      <span className="meta-label">{label}</span>
      <span className="meta-value" title={value}>{value}</span>
    </div>
  );
}

function FirstPromptItem({ prompt }: { prompt: string }) {
  const [expanded, setExpanded] = useState(() => prompt.length <= 900 && prompt.split(/\r?\n/).length <= 18);
  return (
    <article className="timeline-item role-user timeline-start">
      <div className="timeline-left">
        <span className="timeline-time">start</span>
        <span className="timeline-marker" aria-hidden="true">{roleMeta.user.icon}</span>
      </div>
      <div className="timeline-card">
        <header className="message-head">
          <div className="message-title">
            <span className="role-badge">first prompt</span>
          </div>
          <button className="collapse-toggle" onClick={() => setExpanded((value) => !value)} type="button" aria-expanded={expanded}>
            {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
            {expanded ? "Collapse" : "Expand"}
          </button>
        </header>
        {expanded ? <p className="prompt-body">{prompt}</p> : <p className="message-excerpt">{excerpt(prompt)}</p>}
      </div>
    </article>
  );
}

function Message({ message, index }: { message: SessionDetail["transcript"][number]; index: number }) {
  const role = transcriptRole(message.display_role);
  const [expanded, setExpanded] = useState(() => !shouldCollapseMessage(message));
  const [mode, setMode] = useState<MessageMode>("preview");
  const roleLabel = roleMeta[role].label;
  const time = message.timestamp_label && message.timestamp_label !== "-" ? message.timestamp_label : `#${index + 1}`;

  return (
    <article className={`timeline-item role-${role}`}>
      <div className="timeline-left">
        <span className="timeline-time">{time}</span>
        <span className="timeline-marker" aria-hidden="true">{roleMeta[role].icon}</span>
      </div>
      <div className="timeline-card">
        <header className="message-head">
          <div className="message-title">
            <span className="role-badge">{roleLabel}</span>
          </div>
          <div className="message-actions">
            {expanded && (
              <div className="mode-toggle" aria-label={`${roleLabel} display mode`}>
                <button className={mode === "preview" ? "active" : ""} onClick={() => setMode("preview")} type="button">
                  <Eye size={14} />
                  Preview
                </button>
                <button className={mode === "raw" ? "active" : ""} onClick={() => setMode("raw")} type="button">
                  <Code2 size={14} />
                  Raw
                </button>
              </div>
            )}
            <button className="collapse-toggle" onClick={() => setExpanded((value) => !value)} type="button" aria-expanded={expanded}>
              {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
              {expanded ? "Collapse" : "Expand"}
            </button>
          </div>
        </header>
        {expanded ? (
          <div className="message-body">
            {mode === "preview" ? <MarkdownPreview text={message.text} /> : <pre className="raw-message">{message.text}</pre>}
          </div>
        ) : (
          <p className="message-excerpt">{excerpt(message.text)}</p>
        )}
      </div>
    </article>
  );
}

function MarkdownPreview({ text }: { text: string }) {
  return (
    <div className="markdown-preview">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}

function ConfigView({ client, theme, onThemeChange }: { client: ApiClient; theme: Theme; onThemeChange: (theme: Theme) => void }) {
  const [config, setConfig] = useState<LoadState<ConfigSummary>>({ status: "loading" });
  const [health, setHealth] = useState<LoadState<HealthResponse>>({ status: "loading" });
  const [aiDraft, setAiDraft] = useState({ enabled: false, baseUrl: "", model: "", apiKeyEnv: "", apiKey: "" });
  const [aiStatus, setAiStatus] = useState("");

  useEffect(() => {
    setConfig({ status: "loading" });
    setHealth({ status: "loading" });
    client.configSummary().then((data) => setConfig({ status: "ready", data })).catch((error: Error) => setConfig({ status: "error", error: error.message }));
    client.health().then((data) => setHealth({ status: "ready", data })).catch((error: Error) => setHealth({ status: "error", error: error.message }));
  }, [client]);

  if (config.status === "loading" || config.status === "idle") return <Loading title="Config" />;
  if (config.status === "error") return <ErrorPanel title="Config" error={config.error} />;
  if (config.status !== "ready") return <Loading title="Config" />;

  const data = config.data;

  function saveAi(clearApiKey = false) {
    setAiStatus("Saving...");
    client.updateAiConfig({
      enabled: aiDraft.enabled,
      provider: "openai_compatible",
      base_url: aiDraft.baseUrl,
      model: aiDraft.model,
      api_key_env: aiDraft.apiKeyEnv,
      api_key: clearApiKey ? undefined : aiDraft.apiKey,
      clear_api_key: clearApiKey
    }).then((ai) => {
      setConfig((current) => current.status === "ready" ? { status: "ready", data: { ...current.data, ai } } : current);
      setAiDraft({ enabled: ai.enabled, baseUrl: ai.base_url, model: ai.model, apiKeyEnv: ai.api_key_env, apiKey: "" });
      setAiStatus(clearApiKey ? "Stored key cleared." : "AI config saved.");
    }).catch((error: Error) => setAiStatus(error.message));
  }

  return (
    <div className="stack">
      <header className="page-head">
        <div>
          <p>runtime</p>
          <h1>Instance config</h1>
        </div>
        <div className="metric-strip">
          <Metric icon={<Activity size={16} />} label="sessions" value={data.counts.total} />
          <Metric icon={<Cable size={16} />} label="remotes" value={data.remotes.length} />
        </div>
      </header>
      {data.warnings.length > 0 && <Warnings warnings={data.warnings} />}
      <section className="grid-two">
        <section className="panel appearance-panel">
          <h2>Appearance</h2>
          <div className="theme-control" role="group" aria-label="Theme">
            <button className={theme === "dark" ? "active" : ""} onClick={() => onThemeChange("dark")} type="button" aria-pressed={theme === "dark"}>
              <Moon size={16} />
              Dark
            </button>
            <button className={theme === "light" ? "active" : ""} onClick={() => onThemeChange("light")} type="button" aria-pressed={theme === "light"}>
              <Sun size={16} />
              Light
            </button>
          </div>
        </section>
        <InfoPanel title="Service" rows={[
          ["web bind", data.bind],
          ["core bind", data.core_bind],
          ["share base", data.share.base_url],
          ["share token", data.share.token_configured ? "set" : "missing"]
        ]} />
        <InfoPanel title="Terminal stream" rows={health.status === "ready" ? [
          ["protocol", health.data.stream.protocol],
          ["client events", health.data.stream.client_events.join(", ")],
          ["server events", health.data.stream.server_events.join(", ")]
        ] : [["status", "unavailable"]]} />
      </section>
      <AiConfigPanel
        summary={data.ai}
        draft={aiDraft}
        onDraftChange={setAiDraft}
        status={aiStatus}
        onSave={() => saveAi(false)}
        onClearKey={() => saveAi(true)}
      />
      <section className="panel">
        <h2>Remotes</h2>
        <div className="table">
          {data.remotes.length === 0 && <p className="muted">No remotes configured.</p>}
          {data.remotes.map((remote) => (
            <div className="table-row" key={remote.name}>
              <strong>{remote.name}</strong>
              <span>{remote.base_url}</span>
              <em>{remote.enabled ? "enabled" : "disabled"} / {remote.visible ? "visible" : "hidden"} / {remote.session_count} sessions</em>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

function AiConfigPanel({ summary, draft, onDraftChange, status, onSave, onClearKey }: {
  summary: ConfigSummary["ai"];
  draft: { enabled: boolean; baseUrl: string; model: string; apiKeyEnv: string; apiKey: string };
  onDraftChange: (draft: { enabled: boolean; baseUrl: string; model: string; apiKeyEnv: string; apiKey: string }) => void;
  status: string;
  onSave: () => void;
  onClearKey: () => void;
}) {
  useEffect(() => {
    onDraftChange({
      enabled: summary.enabled,
      baseUrl: summary.base_url,
      model: summary.model,
      apiKeyEnv: summary.api_key_env,
      apiKey: ""
    });
  }, [summary.enabled, summary.base_url, summary.model, summary.api_key_env]);

  return (
    <section className="panel ai-panel">
      <div className="panel-head">
        <div>
          <h2>AI</h2>
          <p>OpenAI-compatible summary configuration</p>
        </div>
        <span className="status-pill">{summary.key_source}</span>
      </div>
      <div className="ai-form">
        <label className="check-line">
          <input type="checkbox" checked={draft.enabled} onChange={(event) => onDraftChange({ ...draft, enabled: event.target.checked })} />
          <span>Enable future session summaries</span>
        </label>
        <label>
          <span>Base URL</span>
          <input value={draft.baseUrl} onChange={(event) => onDraftChange({ ...draft, baseUrl: event.target.value })} />
        </label>
        <label>
          <span>Model</span>
          <input value={draft.model} onChange={(event) => onDraftChange({ ...draft, model: event.target.value })} />
        </label>
        <label>
          <span>API key env</span>
          <input value={draft.apiKeyEnv} onChange={(event) => onDraftChange({ ...draft, apiKeyEnv: event.target.value })} />
        </label>
        <label>
          <span>Stored API key</span>
          <input type="password" value={draft.apiKey} placeholder={summary.api_key_configured ? "configured; leave blank to keep" : "optional"} onChange={(event) => onDraftChange({ ...draft, apiKey: event.target.value })} />
        </label>
      </div>
      <div className="panel-actions">
        <button className="icon-line" type="button" onClick={onSave}><KeyRound size={16} />Save AI</button>
        <button className="icon-line secondary" type="button" onClick={onClearKey}>Clear stored key</button>
        {status && <span className="save-status">{status}</span>}
      </div>
    </section>
  );
}

const roleMeta: Record<TranscriptRole, { label: string; icon: ReactNode }> = {
  user: { label: "user", icon: <UserRound size={15} /> },
  assistant: { label: "assistant", icon: <Bot size={15} /> },
  context: { label: "context", icon: <FileText size={15} /> },
  event: { label: "event", icon: <Activity size={15} /> }
};

function transcriptRole(displayRole: string): TranscriptRole {
  if (displayRole === "user" || displayRole === "assistant" || displayRole === "context") return displayRole;
  return "event";
}

function shouldCollapseMessage(message: SessionDetail["transcript"][number]): boolean {
  const role = transcriptRole(message.display_role);
  return role === "context" || role === "event" || message.text.length > 2400 || message.text.split(/\r?\n/).length > 60;
}

function isFirstPromptDuplicate(firstPrompt: string, message: SessionDetail["transcript"][number]): boolean {
  return transcriptRole(message.display_role) === "user" && normalizeText(firstPrompt) === normalizeText(message.text);
}

function normalizeText(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function excerpt(text: string): string {
  const normalized = normalizeText(text);
  if (!normalized) return "Empty message.";
  return normalized.length > 260 ? `${normalized.slice(0, 260)}...` : normalized;
}

function InfoPanel({ title, rows }: { title: string; rows: Array<[string, string]> }) {
  return (
    <section className="panel">
      <h2>{title}</h2>
      <dl className="info-list">
        {rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
      </dl>
    </section>
  );
}

function Metric({ icon, label, value }: { icon: ReactNode; label: string; value: number }) {
  return <div className="metric">{icon}<span>{label}</span><strong>{value}</strong></div>;
}

function Warnings({ warnings }: { warnings: string[] }) {
  return <section className="warnings"><CircleAlert size={16} /><ul>{warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul></section>;
}

function Loading({ title }: { title: string }) {
  return <section className="center-panel"><Activity className="spin" size={18} /><h1>{title}</h1></section>;
}

function ErrorPanel({ title, error }: { title: string; error: string }) {
  return <section className="center-panel error"><CircleAlert size={18} /><h1>{title}</h1><p>{error}</p></section>;
}

function routeFromHash(): View {
  const hash = window.location.hash.replace(/^#\/?/, "");
  if (hash === "config") return { name: "config" };
  const parts = hash.split("/");
  if (parts[0] === "session" && parts.length === 4) {
    return {
      name: "detail",
      ref: {
        origin: decodeURIComponent(parts[1]),
        provider: decodeURIComponent(parts[2]),
        id: decodeURIComponent(parts[3])
      }
    };
  }
  return { name: "sessions" };
}

function readTheme(): Theme {
  const stored = window.localStorage.getItem("coca-web-theme");
  if (stored === "dark" || stored === "light") return stored;
  return "light";
}

function encodePart(value: string): string {
  return encodeURIComponent(value);
}

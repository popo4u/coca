import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
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
  GitBranch,
  KeyRound,
  LayoutDashboard,
  ListFilter,
  LogOut,
  MonitorUp,
  Moon,
  Plug,
  Search,
  Settings,
  Share2,
  ShieldCheck,
  Sun,
  TerminalSquare,
  UserRound,
  Workflow
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  ApiClient,
  clearTerminalToken,
  clearToken,
  readTerminalToken,
  readToken,
  saveTerminalToken,
  saveToken
} from "../api/client";
import type {
  AccountMe,
  AccountUser,
  ConfigSummary,
  HealthResponse,
  SessionDetail,
  SessionRef,
  SessionSummary,
  SessionsResponse,
  TerminalSessionSummary,
  TerminalSessionsResponse
} from "../api/types";
import { AuthGate } from "./AuthGate";
import { ProfileView } from "./ProfileView";
import { TerminalPanel } from "./TerminalPanel";

type View =
  | { name: "dashboard" }
  | { name: "sessions" }
  | { name: "origins" }
  | { name: "terminals" }
  | { name: "profile" }
  | { name: "settings" }
  | { name: "detail"; ref: SessionRef; terminalId?: string | null };

type LoadState<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; data: T }
  | { status: "error"; error: string };

type Theme = "dark" | "light";
type Accent = "pink" | "klein" | "ubuntu" | "cyberpunk" | "black";
type Density = "compact" | "normal" | "comfortable";
type TranscriptRole = "user" | "assistant" | "context" | "event";
type MessageMode = "preview" | "raw";
type AccountState =
  | { status: "loading" }
  | { status: "account"; data: AccountMe }
  | { status: "legacy"; error: string };

const accentThemes: Array<{ id: Accent; label: string; swatch: string }> = [
  { id: "pink", label: "Pink", swatch: "#a64f73" },
  { id: "klein", label: "Klein blue", swatch: "#002fa7" },
  { id: "ubuntu", label: "Ubuntu purple", swatch: "#77216f" },
  { id: "cyberpunk", label: "Cyberpunk yellow", swatch: "#c99a00" },
  { id: "black", label: "Black", swatch: "#151515" }
];

const navItems: Array<{ view: View["name"]; href: string; label: string; icon: ReactNode }> = [
  { view: "dashboard", href: "#/", label: "Dashboard", icon: <LayoutDashboard size={16} /> },
  { view: "sessions", href: "#/sessions", label: "Sessions", icon: <FileText size={16} /> },
  { view: "origins", href: "#/origins", label: "Origins", icon: <Workflow size={16} /> },
  { view: "terminals", href: "#/terminals", label: "Active Terminals", icon: <TerminalSquare size={16} /> },
  { view: "profile", href: "#/profile", label: "Profile", icon: <UserRound size={16} /> },
  { view: "settings", href: "#/settings", label: "Settings", icon: <Settings size={16} /> }
];

export function App() {
  const [token, setToken] = useState(readToken);
  const [terminalToken, setTerminalToken] = useState(readTerminalToken);
  const [view, setView] = useState<View>(() => routeFromHash());
  const [theme, setTheme] = useState<Theme>(() => readTheme());
  const [accent, setAccent] = useState<Accent>(() => readAccent());
  const [density, setDensity] = useState<Density>(() => readDensity());
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [accountState, setAccountState] = useState<AccountState>({ status: "loading" });
  const client = useMemo(() => new ApiClient(token), [token]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.accent = accent;
    document.documentElement.dataset.density = density;
    window.localStorage.setItem("coca-web-theme", theme);
    window.localStorage.setItem("coca-web-accent", accent);
    window.localStorage.setItem("coca-web-density", density);
  }, [theme, accent, density]);

  useEffect(() => {
    const onHash = () => {
      setView(routeFromHash());
      setMobileNavOpen(false);
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  useEffect(() => {
    if (!token) {
      setAccountState({ status: "loading" });
      return;
    }
    setAccountState({ status: "legacy", error: "Account APIs are not available in this gateway build." });
  }, [token]);

  if (!token) {
    return (
      <AuthGate onAuthenticated={(value) => {
        saveToken(value);
        setToken(value);
      }} />
    );
  }

  const page = pageCopy(view);

  return (
    <Shell
      view={view}
      pageTitle={page.title}
      pageSubtitle={page.subtitle}
      mobileNavOpen={mobileNavOpen}
      onToggleMobileNav={() => setMobileNavOpen((open) => !open)}
      accountState={accountState}
      onLogout={() => {
        if (accountState.status === "account") {
          client.logout().catch(() => undefined);
        }
        clearToken();
        clearTerminalToken();
        setToken("");
        setTerminalToken("");
      }}
    >
      {view.name === "dashboard" && <DashboardView client={client} terminalToken={terminalToken} onOpen={openDetail} />}
      {view.name === "sessions" && <SessionsView client={client} onOpen={openDetail} />}
      {view.name === "origins" && <OriginsView client={client} />}
      {view.name === "terminals" && (
        <TerminalsView
          client={client}
          terminalToken={terminalToken}
          onTerminalTokenChange={setTerminalToken}
          onOpenSession={(ref, terminalId) => navigateToDetail(ref, terminalId)}
        />
      )}
      {view.name === "profile" && (
        <ProfileView
          client={client}
          account={accountState.status === "account" ? accountState.data : null}
          legacyReason={accountState.status === "legacy" ? accountState.error : undefined}
          onUserChange={updateAccountUser}
        />
      )}
      {view.name === "settings" && (
        <SettingsView
          client={client}
          theme={theme}
          accent={accent}
          density={density}
          onThemeChange={setTheme}
          onAccentChange={setAccent}
          onDensityChange={setDensity}
        />
      )}
      {view.name === "detail" && (
        <DetailView
          client={client}
          readToken={token}
          terminalToken={terminalToken}
          onTerminalTokenChange={setTerminalToken}
          reference={view.ref}
          initialTerminalId={view.terminalId ?? null}
        />
      )}
    </Shell>
  );

  function openDetail(session: SessionSummary) {
    navigateToDetail({ origin: session.origin, provider: session.provider, id: session.id });
  }

  function navigateToDetail(ref: SessionRef, terminalId: string | null = null) {
    const attach = terminalId ? `?terminal=${encodePart(terminalId)}` : "";
    window.location.hash = `session/${encodePart(ref.origin)}/${encodePart(ref.provider)}/${encodePart(ref.id)}${attach}`;
    setView({ name: "detail", ref, terminalId });
  }

  function updateAccountUser(user: AccountUser) {
    setAccountState((current) => current.status === "account" ? { status: "account", data: { ...current.data, user } } : current);
  }
}

function Shell({
  children,
  view,
  pageTitle,
  pageSubtitle,
  mobileNavOpen,
  onToggleMobileNav,
  accountState,
  onLogout
}: {
  children: ReactNode;
  view: View;
  pageTitle: string;
  pageSubtitle: string;
  mobileNavOpen: boolean;
  onToggleMobileNav: () => void;
  accountState: AccountState;
  onLogout: () => void;
}) {
  const identity = identityDisplay(accountState);
  return (
    <main className="app-shell">
      <div className="mobile-top">
        <button className="btn ghost" type="button" onClick={onToggleMobileNav}>Menu</button>
        <b>coca</b>
        <span className="small muted">gateway</span>
      </div>
      <aside className={`sidebar ${mobileNavOpen ? "open" : ""}`}>
        <a className="side-brand" href="#/">
          <div className="mark">c</div>
          <div>
            <b>coca</b>
            <span>session workspace</span>
          </div>
        </a>
        <nav className="nav" aria-label="Workspace">
          {navItems.map((item) => (
            <a className={isNavActive(view, item.view) ? "active" : ""} href={item.href} key={item.view}>
              <span className="ico">{item.icon}</span>
              <span>{item.label}</span>
            </a>
          ))}
        </nav>
        <div className="side-footer">
          <div className="strong">local gateway</div>
          <div>Browser view, daemon-owned runtime</div>
          <button className="btn small-btn" type="button" onClick={onLogout}><LogOut size={14} /> Sign out</button>
        </div>
      </aside>
      <header className="workspace-header">
        <div className="page-title">
          <h1>{pageTitle}</h1>
          <span>{pageSubtitle}</span>
        </div>
        <a className="global-search" href="#/sessions">
          <Search size={15} />
          <span>Search sessions, cwd, model</span>
        </a>
        <div className="env-chip"><span className="dot" /><span>gateway</span><span className="muted mono">local</span></div>
        <a className="identity" href="#/profile">
          <div className="avatar">{identity.initials}</div>
          <div className="truncate"><b className="truncate">{identity.name}</b><div className="small muted truncate">{identity.detail}</div></div>
        </a>
      </header>
      <section className="workspace">{children}</section>
    </main>
  );
}

function DashboardView({ client, terminalToken, onOpen }: { client: ApiClient; terminalToken: string; onOpen: (session: SessionSummary) => void }) {
  const [sessions, setSessions] = useState<LoadState<SessionsResponse>>({ status: "loading" });
  const [config, setConfig] = useState<LoadState<ConfigSummary>>({ status: "loading" });
  const [terminals, setTerminals] = useState<LoadState<TerminalSessionsResponse>>({ status: terminalToken ? "loading" : "idle" });

  useEffect(() => {
    setSessions({ status: "loading" });
    client.sessions().then((data) => setSessions({ status: "ready", data })).catch((error: Error) => setSessions({ status: "error", error: error.message }));
    setConfig({ status: "loading" });
    client.configSummary().then((data) => setConfig({ status: "ready", data })).catch((error: Error) => setConfig({ status: "error", error: error.message }));
  }, [client]);

  useEffect(() => {
    if (!terminalToken) {
      setTerminals({ status: "idle" });
      return;
    }
    setTerminals({ status: "loading" });
    client.terminalSessions(terminalToken).then((data) => setTerminals({ status: "ready", data })).catch((error: Error) => setTerminals({ status: "error", error: error.message }));
  }, [client, terminalToken]);

  const sessionData = sessions.status === "ready" ? sessions.data : null;
  const configData = config.status === "ready" ? config.data : null;
  const terminalData = terminals.status === "ready" ? terminals.data.terminals : [];
  const recentSessions = sessionData?.sessions.slice(0, 6) ?? [];

  return (
    <div className="grid-12">
      <Module className="span-7" title="Recent Sessions" icon={<FileText size={16} />} action={<a className="btn small-btn" href="#/sessions">View all</a>}>
        {sessions.status === "loading" && <SkeletonRows count={5} />}
        {sessions.status === "error" && <Notice tone="error" title="Sessions unavailable" body={sessions.error} />}
        {recentSessions.length === 0 && sessions.status === "ready" && <EmptyState title="No sessions" body="No provider histories were loaded by this gateway." />}
        <div className="list">
          {recentSessions.map((session) => (
            <button className="list-row session-mini" type="button" key={sessionKey(session)} onClick={() => onOpen(session)}>
              <span className="truncate"><b>{session.title}</b><span className="cwd truncate">{session.cwd}</span></span>
              <span className="badge-row"><ProviderBadge provider={session.provider} /><OriginBadge origin={session.origin} /><TerminalCapabilityBadge session={session} /></span>
            </button>
          ))}
        </div>
      </Module>
      <Module className="span-5" title="Active Terminals" icon={<TerminalSquare size={16} />} action={<a className="btn small-btn" href="#/terminals">Manage</a>}>
        {!terminalToken && <Notice tone="warning" title="Terminal token missing" body="Save a terminal token in Active Terminals or Settings to inspect daemon-owned runtime objects." />}
        {terminals.status === "loading" && <SkeletonRows count={4} />}
        {terminals.status === "error" && <Notice tone="error" title="Terminal registry unavailable" body={terminals.error} />}
        {terminalData.length === 0 && terminals.status === "ready" && <EmptyState title="No active terminals" body="The daemon registry returned no terminal sessions." />}
        <div className="list">
          {terminalData.slice(0, 5).map((terminal) => (
            <div className="list-row terminal-mini" key={terminal.terminal_id}>
              <span className="truncate"><b className="mono">{terminal.terminal_id}</b><span className="small muted">{terminal.session.provider} / {terminal.session.origin}</span></span>
              <StatusBadge label={terminal.state.toLowerCase()} tone={terminalTone(terminal.state)} />
              <a className="btn small-btn" href={`#/session/${encodePart(terminal.session.origin)}/${encodePart(terminal.session.provider)}/${encodePart(terminal.session.id)}?terminal=${encodePart(terminal.terminal_id)}`}>Attach</a>
            </div>
          ))}
        </div>
      </Module>
      <Module className="span-4" title="Runtime Boundary" icon={<Workflow size={16} />}>
        <div className="provenance-strip"><span>Browser</span><b>&gt;</b><span>Gateway</span><b>&gt;</b><span>Daemon</span></div>
        <p className="muted">Transcript browsing stays read-only. Terminal actions route through gateway authorization and daemon runtime ownership.</p>
      </Module>
      <Module className="span-4" title="Service State" icon={<Activity size={16} />}>
        {config.status === "loading" && <SkeletonRows count={3} />}
        {config.status === "error" && <Notice tone="error" title="Config unavailable" body={config.error} />}
        {configData && (
          <dl className="meta-grid">
            <dt>Gateway bind</dt><dd className="mono truncate">{configData.gateway_bind}</dd>
            <dt>Share token</dt><dd>{configData.share.token_configured ? "configured" : "missing"}</dd>
            <dt>Terminal</dt><dd>{configData.terminal.enabled ? "enabled" : "disabled"}</dd>
            <dt>Remotes</dt><dd>{configData.remotes.length}</dd>
          </dl>
        )}
      </Module>
      <Module className="span-4" title="State Coverage" icon={<ShieldCheck size={16} />}>
        <Notice tone="success" title="Ready" body="Sessions and configuration load through gateway APIs." />
        <Notice tone="warning" title="Browse-only" body="Remote or unsupported sessions keep transcript access and disable runtime actions with reasons." />
      </Module>
    </div>
  );
}

function SessionsView({ client, onOpen }: { client: ApiClient; onOpen: (session: SessionSummary) => void }) {
  const [state, setState] = useState<LoadState<SessionsResponse>>({ status: "loading" });
  const [query, setQuery] = useState("");
  const [provider, setProvider] = useState("all");
  const [origin, setOrigin] = useState("all");
  const [model, setModel] = useState("all");
  const [readiness, setReadiness] = useState("all");
  const [sort, setSort] = useState("recent");

  useEffect(() => {
    setState({ status: "loading" });
    client.sessions().then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Sessions" />;
  if (state.status === "error") return <ErrorPanel title="Sessions" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Sessions" />;

  const data = state.data;
  const providers = unique(data.sessions.map((session) => session.provider));
  const origins = unique(data.sessions.map((session) => session.origin));
  const models = unique(data.sessions.map((session) => session.model ?? "-"));
  const filtered = data.sessions.filter((session) => {
    const haystack = `${session.origin} ${session.provider} ${session.title} ${session.cwd} ${session.model ?? ""}`.toLowerCase();
    const readinessLabel = terminalReadiness(session).toLowerCase();
    return (
      (!query.trim() || haystack.includes(query.trim().toLowerCase())) &&
      (provider === "all" || session.provider === provider) &&
      (origin === "all" || session.origin === origin) &&
      (model === "all" || (session.model ?? "-") === model) &&
      (readiness === "all" || readinessLabel === readiness)
    );
  }).sort((left, right) => {
    if (sort === "provider") return left.provider.localeCompare(right.provider);
    if (sort === "messages") return right.message_count - left.message_count;
    return (right.updated_at_ms ?? 0) - (left.updated_at_ms ?? 0);
  });

  return (
    <div className="stack wide-stack">
      <PageMetrics
        items={[
          { icon: <Database size={16} />, label: "total", value: data.counts.total },
          { icon: <Cpu size={16} />, label: "providers", value: providers.length },
          { icon: <Cable size={16} />, label: "origins", value: origins.length }
        ]}
      />
      {data.warnings.length > 0 && <Warnings warnings={data.warnings} />}
      <section className="toolbar">
        <div className="filter-row">
          <label className="filter-search"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search title, cwd, model" /></label>
          <Select value={provider} onChange={setProvider} options={["all", ...providers]} label="Provider" />
          <Select value={origin} onChange={setOrigin} options={["all", ...origins]} label="Origin" />
          <Select value={model} onChange={setModel} options={["all", ...models]} label="Model" />
          <Select value={readiness} onChange={setReadiness} options={["all", "terminal ready", "browse only"]} label="Readiness" />
        </div>
        <div className="filter-row compact-row"><ListFilter size={15} /><Select value={sort} onChange={setSort} options={["recent", "provider", "messages"]} label="Sort" /></div>
      </section>
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Title</th>
              <th>Provider</th>
              <th>Origin</th>
              <th>cwd</th>
              <th>Model</th>
              <th>Updated</th>
              <th>Msgs</th>
              <th>Terminal</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((session) => (
              <tr key={sessionKey(session)} onClick={() => onOpen(session)}>
                <td><div className="truncate"><b>{session.title}</b></div><div className="small muted mono truncate">{session.id}</div></td>
                <td><ProviderBadge provider={session.provider} /></td>
                <td><OriginBadge origin={session.origin} /></td>
                <td><div className="cwd truncate">{session.cwd}</div></td>
                <td><div className="truncate">{session.model ?? "-"}</div></td>
                <td>{session.updated_label}</td>
                <td className="count">{session.message_count}</td>
                <td><TerminalCapabilityBadge session={session} /></td>
              </tr>
            ))}
          </tbody>
        </table>
        {filtered.length === 0 && <EmptyState title="Filtered empty" body="No sessions match the current search, provider, origin, model, or readiness filters." />}
      </div>
    </div>
  );
}

function DetailView({
  client,
  readToken,
  terminalToken,
  onTerminalTokenChange,
  reference,
  initialTerminalId
}: {
  client: ApiClient;
  readToken: string;
  terminalToken: string;
  onTerminalTokenChange: (token: string) => void;
  reference: SessionRef;
  initialTerminalId: string | null;
}) {
  const [state, setState] = useState<LoadState<SessionDetail>>({ status: "loading" });
  const [share, setShare] = useState("");

  useEffect(() => {
    setState({ status: "loading" });
    setShare("");
    client.session(reference).then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client, reference.origin, reference.provider, reference.id]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Session" />;
  if (state.status === "error") return <ErrorPanel title="Session" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Session" />;

  const { summary, transcript } = state.data;
  const firstPrompt = summary.first_user_message?.trim() || "";
  const transcriptMessages = firstPrompt && transcript[0] && isFirstPromptDuplicate(firstPrompt, transcript[0]) ? transcript.slice(1) : transcript;

  return (
    <div className="stack wide-stack detail-stack">
      <header className="detail-header">
        <div className="truncate">
          <h2 title={summary.title}>{summary.title}</h2>
          <div className="badge-row">
            <ProviderBadge provider={summary.provider} />
            <OriginBadge origin={summary.origin} />
            <StatusBadge label="read-only transcript" tone="info" />
            <TerminalCapabilityBadge session={summary} />
            <span className="tag">{summary.model ?? "-"}</span>
            <span className="small muted">Updated {summary.updated_label}</span>
          </div>
        </div>
        <div className="detail-actions">
          <button className="btn small-btn" type="button" onClick={() => exportTranscript(summary, transcriptMessages, firstPrompt)}>Export raw</button>
          <button className="btn primary small-btn" type="button" onClick={() => client.shareSession(reference).then((link) => setShare(link.url)).catch((error: Error) => setShare(error.message))}><Share2 size={14} /> Share</button>
        </div>
      </header>
      {share && <div className="notice"><code>{share}</code></div>}
      <TerminalPanel
        client={client}
        readToken={readToken}
        terminalToken={terminalToken}
        onTerminalTokenChange={onTerminalTokenChange}
        session={summary}
        reference={reference}
        initialAttachId={initialTerminalId}
      />
      <div className="detail-layout">
        <section className="transcript-shell" aria-label="Transcript">
          <header className="transcript-toolbar">
            <div><b>Transcript Timeline</b> <span className="small muted">read-only execution history</span></div>
            <span className="tag">transcript.read</span>
          </header>
          <div className="timeline">
            {!firstPrompt && transcriptMessages.length === 0 && <EmptyState title="No transcript reconstructed" body="The provider history was found, but no ordered user or assistant text could be rebuilt." />}
            {firstPrompt && <FirstPromptItem prompt={firstPrompt} />}
            {transcriptMessages.map((message, index: number) => <Message key={`${message.timestamp_ms ?? index}:${index}`} message={message} index={index + (firstPrompt ? 1 : 0)} />)}
          </div>
        </section>
        <aside className="context-panel">
          <Module title="Metadata Summary" icon={<Database size={16} />}>
            <dl className="meta-grid">
              <dt>Session id</dt><dd className="mono truncate">{summary.id}</dd>
              <dt>Provider</dt><dd>{summary.provider}</dd>
              <dt>Origin</dt><dd>{summary.origin}</dd>
              <dt>cwd</dt><dd className="mono truncate">{summary.cwd}</dd>
              <dt>Messages</dt><dd>{summary.message_count}</dd>
              <dt>Scope</dt><dd>transcript.read</dd>
            </dl>
          </Module>
          <Module title="Runtime Readiness" icon={<TerminalSquare size={16} />} action={<TerminalCapabilityBadge session={summary} />}>
            <Notice
              tone={summary.terminal.enabled ? "success" : "warning"}
              title={terminalReadiness(summary)}
              body={summary.terminal.unavailable_message ?? "Resume and fork requests go through gateway authorization and daemon runtime ownership."}
            />
            <div className="provenance-strip"><span>Browser</span><b>&gt;</b><span>Gateway</span><b>&gt;</b><span>Daemon</span></div>
          </Module>
          <Module title="Share / Read-only Scope" icon={<ShieldCheck size={16} />}>
            <p className="muted">Shared views include transcript, metadata, and provenance. They do not include terminal write tokens.</p>
          </Module>
        </aside>
      </div>
    </div>
  );
}

function OriginsView({ client }: { client: ApiClient }) {
  const [state, setState] = useState<LoadState<ConfigSummary>>({ status: "loading" });
  const [query, setQuery] = useState("");
  const [visibility, setVisibility] = useState("all");

  useEffect(() => {
    setState({ status: "loading" });
    client.configSummary().then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Origins" />;
  if (state.status === "error") return <ErrorPanel title="Origins" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Origins" />;

  const remotes = state.data.remotes.filter((remote) => {
    const haystack = `${remote.name} ${remote.base_url}`.toLowerCase();
    return (!query.trim() || haystack.includes(query.trim().toLowerCase())) &&
      (visibility === "all" || (visibility === "visible" ? remote.visible : !remote.visible));
  });

  return (
    <div className="stack wide-stack">
      <section className="toolbar">
        <div className="filter-row">
          <label className="filter-search"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search remote name or URL" /></label>
          <Select value={visibility} onChange={setVisibility} options={["all", "visible", "hidden"]} label="Visibility" />
        </div>
      </section>
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Base URL</th>
              <th>Enabled</th>
              <th>Visible</th>
              <th>Terminal</th>
              <th>Sessions</th>
            </tr>
          </thead>
          <tbody>
            {remotes.map((remote) => (
              <tr key={remote.name}>
                <td><b>{remote.name}</b><div className="small muted">remote summary</div></td>
                <td><div className="cwd truncate">{remote.base_url}</div></td>
                <td><StatusBadge label={remote.enabled ? "enabled" : "disabled"} tone={remote.enabled ? "success" : "warning"} /></td>
                <td><StatusBadge label={remote.visible ? "visible" : "hidden"} tone={remote.visible ? "success" : "warning"} /></td>
                <td><StatusBadge label={remote.terminal_ready ? "terminal ready" : (remote.terminal_unavailable_message ?? "browse-only")} tone={remote.terminal_ready ? "success" : "warning"} /></td>
                <td className="count">{remote.session_count}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {remotes.length === 0 && <EmptyState title="No origins" body="No remotes match the current filters. Fleet install/update/uninstall telemetry is not exposed by the current gateway API." />}
      </div>
      <Notice tone="info" title="API boundary" body="This page maps to configured remote summaries only. Machine telemetry and client lifecycle actions are tracked in .ai/gap.md." />
    </div>
  );
}

function TerminalsView({
  client,
  terminalToken,
  onTerminalTokenChange,
  onOpenSession
}: {
  client: ApiClient;
  terminalToken: string;
  onTerminalTokenChange: (token: string) => void;
  onOpenSession: (ref: SessionRef, terminalId: string) => void;
}) {
  const [draftToken, setDraftToken] = useState("");
  const [state, setState] = useState<LoadState<TerminalSessionsResponse>>({ status: terminalToken ? "loading" : "idle" });
  const [query, setQuery] = useState("");
  const [terminalState, setTerminalState] = useState("all");
  const [attaching, setAttaching] = useState<TerminalSessionSummary | null>(null);
  const attachTimerRef = useRef<number | null>(null);

  useEffect(() => {
    if (!terminalToken) {
      setState({ status: "idle" });
      return;
    }
    setState({ status: "loading" });
    client.terminalSessions(terminalToken).then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client, terminalToken]);

  const terminals = state.status === "ready" ? state.data.terminals.filter((terminal) => {
    const haystack = `${terminal.terminal_id} ${terminal.session.origin} ${terminal.session.provider} ${terminal.session.id} ${terminal.mode} ${terminal.state}`.toLowerCase();
    return (!query.trim() || haystack.includes(query.trim().toLowerCase())) &&
      (terminalState === "all" || terminal.state.toLowerCase() === terminalState);
  }) : [];

  function submitTerminalToken(event: FormEvent) {
    event.preventDefault();
    const token = draftToken.trim();
    if (!token) return;
    saveTerminalToken(token);
    onTerminalTokenChange(token);
    setDraftToken("");
  }

  function clearSavedTerminalToken() {
    clearTerminalToken();
    onTerminalTokenChange("");
    setDraftToken("");
  }

  function clearAttachTimer() {
    if (attachTimerRef.current !== null) {
      window.clearTimeout(attachTimerRef.current);
      attachTimerRef.current = null;
    }
  }

  function beginAttach(terminal: TerminalSessionSummary) {
    clearAttachTimer();
    setAttaching(terminal);
    attachTimerRef.current = window.setTimeout(() => {
      attachTimerRef.current = null;
      onOpenSession(terminal.session, terminal.terminal_id);
      setAttaching(null);
    }, 1300);
  }

  function cancelAttach() {
    clearAttachTimer();
    setAttaching(null);
  }

  useEffect(() => clearAttachTimer, []);

  return (
    <div className="stack wide-stack">
      {attaching && <AttachOverlay terminal={attaching} onCancel={cancelAttach} />}
      <Module title="Terminal Access" icon={<KeyRound size={16} />}>
        <div className="terminal-access compact-access">
          <div>
            <strong>Terminal token</strong>
            <span>{terminalToken ? "saved for this browser" : "required for daemon terminal registry and attach operations"}</span>
          </div>
          {terminalToken ? (
            <button className="btn small-btn" type="button" onClick={clearSavedTerminalToken}>Clear token</button>
          ) : (
            <form className="terminal-token-form" onSubmit={submitTerminalToken}>
              <input type="password" value={draftToken} onChange={(event) => setDraftToken(event.target.value)} placeholder="Terminal token" aria-label="Terminal token" />
              <button className="btn primary small-btn" type="submit">Save</button>
            </form>
          )}
        </div>
      </Module>
      <section className="toolbar">
        <div className="filter-row">
          <label className="filter-search"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search terminal id, session, origin" /></label>
          <Select value={terminalState} onChange={setTerminalState} options={["all", "starting", "running", "detached", "exited"]} label="State" />
        </div>
      </section>
      {state.status === "idle" && <Notice tone="warning" title="Terminal token missing" body="Save a terminal token to load daemon-owned terminal sessions." />}
      {state.status === "loading" && <Loading title="Active Terminals" />}
      {state.status === "error" && <ErrorPanel title="Active Terminals" error={state.error} />}
      {state.status === "ready" && (
        <div className="table-wrap terminal-table">
          <table>
            <thead>
              <tr>
                <th>Terminal</th>
                <th>Session</th>
                <th>Mode</th>
                <th>State</th>
                <th>Clients</th>
                <th>Size</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {terminals.map((terminal) => (
                <tr key={terminal.terminal_id}>
                  <td><b className="mono">{terminal.terminal_id}</b><div className="small muted">daemon runtime</div></td>
                  <td><div className="truncate"><b>{terminal.session.provider}</b> / {terminal.session.origin}</div><div className="small muted mono truncate">{terminal.session.id}</div></td>
                  <td><span className="tag">{terminal.mode.toLowerCase()}</span></td>
                  <td><StatusBadge label={terminal.state.toLowerCase()} tone={terminalTone(terminal.state)} /></td>
                  <td className="count">{terminal.attached_clients}</td>
                  <td className="mono">{terminal.size.cols}x{terminal.size.rows}</td>
                  <td><button className="btn primary small-btn" type="button" onClick={() => beginAttach(terminal)} disabled={terminal.state === "Exited"}><Plug size={14} /> Attach</button></td>
                </tr>
              ))}
            </tbody>
          </table>
          {terminals.length === 0 && <EmptyState title="Filtered empty" body="No terminals match the current search or state filter." />}
        </div>
      )}
      <div className="grid-12">
        <Module className="span-6" title="Runtime unavailable" icon={<CircleAlert size={16} />}>
          <Notice tone="error" title="Daemon unavailable" body="Terminal lifecycle cannot be changed from the browser when the daemon or socket is unavailable. Transcript browsing remains available." />
        </Module>
        <Module className="span-6" title="Runtime Boundary" icon={<Workflow size={16} />}>
          <div className="provenance-strip"><span>Browser</span><b>&gt;</b><span>Gateway</span><b>&gt;</b><span>Daemon</span></div>
        </Module>
      </div>
    </div>
  );
}

function AttachOverlay({ terminal, onCancel }: { terminal: TerminalSessionSummary; onCancel: () => void }) {
  return (
    <div className="attach-overlay active" role="status" aria-live="polite">
      <div className="attach-card">
        <div className="attach-head">
          <div className="head-symbol"><TerminalSquare size={16} /></div>
          <div className="truncate">
            <b>Attaching terminal runtime</b>
            <div className="small muted truncate">{terminal.terminal_id} / {terminal.session.provider} / {terminal.session.origin}</div>
          </div>
          <button className="btn small-btn" type="button" onClick={onCancel}>Cancel</button>
        </div>
        <div className="attach-stage">
          <div className="attach-route">
            <div className="attach-node done"><LayoutDashboard size={16} /><b>Browser</b><span>Request terminal.attach scope</span></div>
            <div className="attach-rail" />
            <div className="attach-node active"><ShieldCheck size={16} /><b>Gateway</b><span>Authorize token and socket</span></div>
            <div className="attach-rail" />
            <div className="attach-node"><Plug size={16} /><b>Daemon</b><span>Own runtime lifecycle</span></div>
          </div>
          <div className="attach-log">
            <div>browser: terminal attach requested</div>
            <div>gateway: validating terminal token and session scope</div>
            <div>daemon: opening pty stream for {terminal.terminal_id}</div>
          </div>
        </div>
      </div>
    </div>
  );
}

function SettingsView({
  client,
  theme,
  accent,
  density,
  onThemeChange,
  onAccentChange,
  onDensityChange
}: {
  client: ApiClient;
  theme: Theme;
  accent: Accent;
  density: Density;
  onThemeChange: (theme: Theme) => void;
  onAccentChange: (accent: Accent) => void;
  onDensityChange: (density: Density) => void;
}) {
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

  if (config.status === "loading" || config.status === "idle") return <Loading title="Settings" />;
  if (config.status === "error") return <ErrorPanel title="Settings" error={config.error} />;
  if (config.status !== "ready") return <Loading title="Settings" />;

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
    <div className="settings-layout">
      <aside className="module settings-index">
        <a href="#settings-preferences">Preferences</a>
        <a href="#settings-runtime">Runtime</a>
        <a href="#settings-ai">AI</a>
        <a href="#settings-remotes">Remotes</a>
      </aside>
      <div className="settings-main">
        {data.warnings.length > 0 && <Warnings warnings={data.warnings} />}
        <Module id="settings-preferences" title="Preferences" icon={<Settings size={16} />} action={<span className="small muted">Saved locally</span>}>
          <div className="setting-row"><div><b>Theme</b><span>Choose the workspace surface mode.</span></div><Segmented value={theme} values={["light", "dark"]} onChange={(value) => onThemeChange(value as Theme)} icons={{ light: <Sun size={15} />, dark: <Moon size={15} /> }} /></div>
          <div className="setting-row"><div><b>Theme color</b><span>Pick the accent used for selected controls and active work states.</span></div><div className="theme-picker">{accentThemes.map((item) => <button className={`theme-choice ${accent === item.id ? "active" : ""}`} style={{ "--swatch": item.swatch } as CSSProperties} type="button" key={item.id} onClick={() => onAccentChange(item.id)}><span className="theme-swatch" /><span>{item.label}</span></button>)}</div></div>
          <div className="setting-row"><div><b>Density</b><span>Adjust row height without changing type hierarchy.</span></div><Segmented value={density} values={["compact", "normal", "comfortable"]} onChange={(value) => onDensityChange(value as Density)} /></div>
        </Module>
        <Module id="settings-runtime" title="Runtime" icon={<TerminalSquare size={16} />}>
          <div className="grid-two">
            <InfoPanel title="Service" rows={[
              ["active bind", data.bind],
              ["configured bind", data.gateway_bind],
              ["share base", data.share.base_url],
              ["share token", data.share.token_configured ? "set" : "missing"]
            ]} />
            <InfoPanel title="Terminal stream" rows={health.status === "ready" ? [
              ["protocol", health.data.stream.protocol],
              ["client events", health.data.stream.client_events.join(", ")],
              ["server events", health.data.stream.server_events.join(", ")]
            ] : [["status", "unavailable"]]} />
            <InfoPanel title="Terminal access" rows={[
              ["enabled", data.terminal.enabled ? "yes" : "no"],
              ["token", data.terminal.token_configured ? "configured" : "missing"],
              ["daemon", data.terminal.daemon_available ? "available" : "unavailable"],
              ["socket", data.terminal.terminal_socket_available ? "available" : "unavailable"],
              ["status", data.terminal.unavailable_message ?? "ready"]
            ]} />
          </div>
        </Module>
        <AiConfigPanel
          summary={data.ai}
          draft={aiDraft}
          onDraftChange={setAiDraft}
          status={aiStatus}
          onSave={() => saveAi(false)}
          onClearKey={() => saveAi(true)}
        />
        <Module id="settings-remotes" title="Remotes" icon={<MonitorUp size={16} />}>
          <div className="table">
            {data.remotes.length === 0 && <p className="muted">No remotes configured.</p>}
            {data.remotes.map((remote) => (
              <div className="table-row" key={remote.name}>
                <strong>{remote.name}</strong>
                <span>{remote.base_url}</span>
                <em>{remote.enabled ? "enabled" : "disabled"} / {remote.visible ? "visible" : "hidden"} / {remote.terminal_ready ? "terminal ready" : (remote.terminal_unavailable_message ?? "browse-only")} / {remote.session_count} sessions</em>
              </div>
            ))}
          </div>
        </Module>
      </div>
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
    <Module id="settings-ai" className="ai-panel" title="AI" icon={<GitBranch size={16} />} action={<span className="status-pill">{summary.key_source}</span>}>
      <div className="ai-form">
        <label className="check-line">
          <input type="checkbox" checked={draft.enabled} onChange={(event) => onDraftChange({ ...draft, enabled: event.target.checked })} />
          <span>Enable future session summaries</span>
        </label>
        <label className="field"><span>Base URL</span><input value={draft.baseUrl} onChange={(event) => onDraftChange({ ...draft, baseUrl: event.target.value })} /></label>
        <label className="field"><span>Model</span><input value={draft.model} onChange={(event) => onDraftChange({ ...draft, model: event.target.value })} /></label>
        <label className="field"><span>API key env</span><input value={draft.apiKeyEnv} onChange={(event) => onDraftChange({ ...draft, apiKeyEnv: event.target.value })} /></label>
        <label className="field"><span>Stored API key</span><input type="password" value={draft.apiKey} placeholder={summary.api_key_configured ? "configured; leave blank to keep" : "optional"} onChange={(event) => onDraftChange({ ...draft, apiKey: event.target.value })} /></label>
      </div>
      <div className="panel-actions">
        <button className="btn primary small-btn" type="button" onClick={onSave}><KeyRound size={14} /> Save AI</button>
        <button className="btn small-btn" type="button" onClick={onClearKey}>Clear stored key</button>
        {status && <span className="save-status">{status}</span>}
      </div>
    </Module>
  );
}

function FirstPromptItem({ prompt }: { prompt: string }) {
  const [expanded, setExpanded] = useState(() => prompt.length <= 900 && prompt.split(/\r?\n/).length <= 18);
  return (
    <article className="timeline-item user">
      <div className="timeline-time">start<br /><span className="muted">#01</span></div>
      <div className="role-marker">u</div>
      <div className="timeline-card">
        <header className="tl-head">
          <div className="tl-title"><span className="role-name">user</span><span>Initial prompt</span><span className="tag">timeline start</span></div>
          <button className="btn small-btn" onClick={() => setExpanded((value) => !value)} type="button" aria-expanded={expanded}>{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />} {expanded ? "Collapse" : "Expand"}</button>
        </header>
        {expanded ? <p className="tl-body prompt-body">{prompt}</p> : <p className="tl-body message-excerpt">{excerpt(prompt)}</p>}
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
    <article className={`timeline-item ${role}`}>
      <div className="timeline-time">{time}<br /><span className="muted">#{String(index + 1).padStart(2, "0")}</span></div>
      <div className="role-marker">{roleLabel[0]}</div>
      <div className="timeline-card">
        <header className="tl-head">
          <div className="tl-title"><span className="role-name">{roleLabel}</span><span className="truncate">{message.display_role}</span></div>
          <div className="message-actions">
            {expanded && (
              <div className="segmented" aria-label={`${roleLabel} display mode`}>
                <button className={mode === "preview" ? "active" : ""} onClick={() => setMode("preview")} type="button"><Eye size={14} /> Preview</button>
                <button className={mode === "raw" ? "active" : ""} onClick={() => setMode("raw")} type="button"><Code2 size={14} /> Raw</button>
              </div>
            )}
            <button className="btn small-btn" onClick={() => setExpanded((value) => !value)} type="button" aria-expanded={expanded}>{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />} {expanded ? "Collapse" : "Expand"}</button>
          </div>
        </header>
        {expanded ? (
          <div className="tl-body">
            {mode === "preview" ? <MarkdownPreview text={message.text} /> : <pre className="raw-message">{message.text}</pre>}
          </div>
        ) : (
          <p className="tl-body message-excerpt">{excerpt(message.text)}</p>
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

function Module({ id, className = "", title, icon, action, children }: { id?: string; className?: string; title: string; icon?: ReactNode; action?: ReactNode; children: ReactNode }) {
  return (
    <section className={`module ${className}`} id={id}>
      <header className="module-head">
        <h2 className="module-title">{icon}<span>{title}</span></h2>
        {action}
      </header>
      <div className="module-body">{children}</div>
    </section>
  );
}

function InfoPanel({ title, rows }: { title: string; rows: Array<[string, string]> }) {
  return (
    <section className="info-panel">
      <h3>{title}</h3>
      <dl className="info-list">
        {rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
      </dl>
    </section>
  );
}

function PageMetrics({ items }: { items: Array<{ icon: ReactNode; label: string; value: number }> }) {
  return (
    <section className="metric-strip">
      {items.map((item) => <Metric icon={item.icon} label={item.label} value={item.value} key={item.label} />)}
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
  return <section className="center-panel"><Activity className="spin" size={18} /><h1>{title}</h1><div className="skeleton" /></section>;
}

function ErrorPanel({ title, error }: { title: string; error: string }) {
  return <section className="center-panel error"><CircleAlert size={18} /><h1>{title}</h1><p>{error}</p></section>;
}

function EmptyState({ title, body }: { title: string; body: string }) {
  return <div className="empty"><b>{title}</b><br />{body}</div>;
}

function Notice({ tone = "info", title, body }: { tone?: "info" | "success" | "warning" | "error"; title: string; body: string }) {
  return <div className={`notice ${tone}`}><b>{title}</b><br />{body}</div>;
}

function Select({ label, value, options, onChange }: { label: string; value: string; options: string[]; onChange: (value: string) => void }) {
  return (
    <label className="select-field">
      <span>{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => <option value={option} key={option}>{optionLabel(option)}</option>)}
      </select>
    </label>
  );
}

function Segmented({ value, values, onChange, icons = {} }: { value: string; values: string[]; onChange: (value: string) => void; icons?: Record<string, ReactNode> }) {
  return (
    <div className="segmented">
      {values.map((item) => (
        <button className={value === item ? "active" : ""} type="button" onClick={() => onChange(item)} key={item}>
          {icons[item]} {optionLabel(item)}
        </button>
      ))}
    </div>
  );
}

function ProviderBadge({ provider }: { provider: string }) {
  return <span className={`provider-badge ${provider.toLowerCase()}`}>{provider}</span>;
}

function OriginBadge({ origin }: { origin: string }) {
  return <span className={`origin-badge ${origin.toLowerCase()}`}>{origin}</span>;
}

function StatusBadge({ label, tone = "info" }: { label: string; tone?: "info" | "success" | "warning" | "error" }) {
  return <span className={`status-badge ${tone}`}>{label}</span>;
}

function TerminalCapabilityBadge({ session }: { session: SessionSummary }) {
  const label = terminalReadiness(session);
  const title = session.terminal.unavailable_message ?? "Terminal resume and fork are available.";
  return <span className={`terminal-badge ${session.terminal.enabled ? "ready" : "blocked"}`} title={title}>{label}</span>;
}

function SkeletonRows({ count }: { count: number }) {
  return <div className="skeleton-list">{Array.from({ length: count }, (_, index) => <div className="skeleton" key={index} />)}</div>;
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

function exportTranscript(summary: SessionSummary, messages: SessionDetail["transcript"], firstPrompt: string) {
  const parts = [
    `# ${summary.title}`,
    "",
    `session: ${summary.origin}/${summary.provider}/${summary.id}`,
    `cwd: ${summary.cwd}`,
    "",
    firstPrompt ? `## first prompt\n\n${firstPrompt}` : "",
    ...messages.map((message) => `## ${message.display_role} ${message.timestamp_label}\n\n${message.text}`)
  ].filter(Boolean);
  const blob = new Blob([parts.join("\n\n")], { type: "text/markdown;charset=utf-8" });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = `${summary.provider}-${summary.id}.md`;
  link.click();
  URL.revokeObjectURL(link.href);
}

function unique(values: string[]) {
  return Array.from(new Set(values)).sort((left, right) => left.localeCompare(right));
}

function sessionKey(session: SessionSummary) {
  return `${session.origin}:${session.provider}:${session.id}`;
}

function terminalReadiness(session: SessionSummary) {
  if (session.terminal.enabled) return "terminal ready";
  return session.terminal.unavailable_code ? optionLabel(session.terminal.unavailable_code.replaceAll("_", " ")) : "browse only";
}

function terminalTone(state: TerminalSessionSummary["state"]): "info" | "success" | "warning" | "error" {
  if (state === "Running") return "success";
  if (state === "Detached" || state === "Starting") return "warning";
  if (state === "Exited") return "error";
  return "info";
}

function pageCopy(view: View) {
  switch (view.name) {
    case "dashboard":
      return { title: "Dashboard", subtitle: "Continue operational work" };
    case "sessions":
      return { title: "Sessions", subtitle: "Browse normalized agent history" };
    case "origins":
      return { title: "Origins", subtitle: "Configured remotes visible to this gateway" };
    case "terminals":
      return { title: "Active Terminals", subtitle: "Daemon-owned runtime objects" };
    case "profile":
      return { title: "Profile", subtitle: "Developer identity, account security, and access tokens" };
    case "settings":
      return { title: "Settings", subtitle: "Preferences, runtime config, and access state" };
    case "detail":
      return { title: "Session Detail", subtitle: "Read transcript and inspect runtime" };
  }
}

function isNavActive(view: View, name: View["name"]) {
  if (name === "sessions" && view.name === "detail") return true;
  return view.name === name;
}

function identityDisplay(accountState: AccountState) {
  if (accountState.status === "account") {
    const user = accountState.data.user;
    return {
      initials: accountInitials(user.display_name || user.email),
      name: user.display_name || user.email,
      detail: user.email
    };
  }
  if (accountState.status === "legacy") {
    return { initials: "lg", name: "Local gateway", detail: "legacy token mode" };
  }
  return { initials: "cx", name: "coca", detail: "checking identity" };
}

function accountInitials(value: string) {
  return value.split(/[^\p{L}\p{N}]+/u).filter(Boolean).slice(0, 2).map((part) => part[0]).join("").toLowerCase() || "cx";
}

function optionLabel(value: string) {
  return value.replaceAll("_", " ").replace(/^\w/, (letter) => letter.toUpperCase());
}

function routeFromHash(): View {
  const rawHash = window.location.hash.replace(/^#\/?/, "");
  const [hash, query = ""] = rawHash.split("?");
  if (!hash || hash === "dashboard") return { name: "dashboard" };
  if (hash === "sessions") return { name: "sessions" };
  if (hash === "origins") return { name: "origins" };
  if (hash === "terminals") return { name: "terminals" };
  if (hash === "profile") return { name: "profile" };
  if (hash === "settings" || hash === "config") return { name: "settings" };
  const parts = hash.split("/");
  if (parts[0] === "session" && parts.length === 4) {
    const params = new URLSearchParams(query);
    return {
      name: "detail",
      ref: {
        origin: decodeURIComponent(parts[1]),
        provider: decodeURIComponent(parts[2]),
        id: decodeURIComponent(parts[3])
      },
      terminalId: params.get("terminal")
    };
  }
  return { name: "dashboard" };
}

function readTheme(): Theme {
  const stored = window.localStorage.getItem("coca-web-theme");
  if (stored === "dark" || stored === "light") return stored;
  return "light";
}

function readAccent(): Accent {
  const stored = window.localStorage.getItem("coca-web-accent");
  return accentThemes.some((theme) => theme.id === stored) ? stored as Accent : "pink";
}

function readDensity(): Density {
  const stored = window.localStorage.getItem("coca-web-density");
  if (stored === "compact" || stored === "comfortable") return stored;
  return "normal";
}

function encodePart(value: string): string {
  return encodeURIComponent(value);
}

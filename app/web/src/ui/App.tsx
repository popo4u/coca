import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import {
  Activity,
  Bot,
  Cable,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  Code2,
  Cpu,
  Database,
  Eye,
  FileText,
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
  clearToken,
  openTerminalSocket,
  readToken,
  saveToken,
  sendTerminalFrame
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
  TerminalMode,
  TerminalServerFrame,
  TerminalSessionSummary,
  TerminalSessionsResponse,
  TerminalSize
} from "../api/types";
import { AuthGate } from "./AuthGate";
import { ProfileView } from "./ProfileView";

const TerminalPanel = lazy(() => import("./TerminalPanel").then((module) => ({ default: module.TerminalPanel })));
const launcherTerminalSize: TerminalSize = { cols: 100, rows: 28 };

type View =
  | { name: "dashboard" }
  | { name: "sessions" }
  | { name: "origins" }
  | { name: "terminals" }
  | { name: "terminalLive"; ref: SessionRef; terminalId: string }
  | { name: "share"; linkId: string; shareToken: string }
  | { name: "profile" }
  | { name: "settings" }
  | { name: "detail"; ref: SessionRef };

type LoadState<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; data: T }
  | { status: "error"; error: string };

type Theme = "dark" | "light";
type Accent = "pink" | "klein" | "ubuntu" | "cyberpunk" | "black";
type Density = "compact" | "normal" | "comfortable";
type Background = "porcelain" | "paper-gray" | "rose-mist";
type TerminalTheme = "one-half-dark" | "campbell" | "vintage" | "solarized-dark" | "tango-dark";
type TranscriptDefault = "rendered" | "raw";
type LandingPage = "dashboard" | "sessions" | "terminals";
type TerminalBehavior = "detach-confirm-kill" | "ask-before-detach";
type TranscriptRole = "user" | "assistant" | "context" | "event";
type MessageMode = "preview" | "raw";
type AccountState =
  | { status: "loading" }
  | { status: "account"; data: AccountMe }
  | { status: "error"; error: string };

const accentThemes: Array<{ id: Accent; label: string; swatch: string }> = [
  { id: "pink", label: "Pink", swatch: "#a64f73" },
  { id: "klein", label: "Klein blue", swatch: "#002fa7" },
  { id: "ubuntu", label: "Ubuntu purple", swatch: "#77216f" },
  { id: "cyberpunk", label: "Cyberpunk yellow", swatch: "#c99a00" },
  { id: "black", label: "Black", swatch: "#151515" }
];

const backgroundOptions: Array<{ id: Background; label: string }> = [
  { id: "porcelain", label: "Porcelain" },
  { id: "paper-gray", label: "Paper gray" },
  { id: "rose-mist", label: "Rose mist" }
];

const terminalThemes: Array<{ id: TerminalTheme; label: string; swatch: string }> = [
  { id: "one-half-dark", label: "One Half Dark", swatch: "#282c34" },
  { id: "campbell", label: "Campbell", swatch: "#0c0c0c" },
  { id: "vintage", label: "Vintage", swatch: "#000000" },
  { id: "solarized-dark", label: "Solarized Dark", swatch: "#002b36" },
  { id: "tango-dark", label: "Tango Dark", swatch: "#171a16" }
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
  const [view, setView] = useState<View>(() => routeFromHash());
  const [theme, setTheme] = useState<Theme>(() => readTheme());
  const [accent, setAccent] = useState<Accent>(() => readAccent());
  const [density, setDensity] = useState<Density>(() => readDensity());
  const [background, setBackground] = useState<Background>(() => readBackground());
  const [terminalTheme, setTerminalTheme] = useState<TerminalTheme>(() => readTerminalTheme());
  const [transcriptDefault, setTranscriptDefault] = useState<TranscriptDefault>(() => readTranscriptDefault());
  const [landingPage, setLandingPage] = useState<LandingPage>(() => readLandingPage());
  const [terminalBehavior, setTerminalBehavior] = useState<TerminalBehavior>(() => readTerminalBehavior());
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [accountState, setAccountState] = useState<AccountState>({ status: "loading" });
  const client = useMemo(() => new ApiClient(token), [token]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.accent = accent;
    document.documentElement.dataset.density = density;
    document.documentElement.dataset.background = background;
    window.localStorage.setItem("coca-web-theme", theme);
    window.localStorage.setItem("coca-web-accent", accent);
    window.localStorage.setItem("coca-web-density", density);
    window.localStorage.setItem("coca-web-background", background);
  }, [theme, accent, density, background]);

  useEffect(() => {
    window.localStorage.setItem("coca-web-terminal-theme", terminalTheme);
    window.localStorage.setItem("coca-web-transcript-default", transcriptDefault);
    window.localStorage.setItem("coca-web-landing-page", landingPage);
    window.localStorage.setItem("coca-web-terminal-behavior", terminalBehavior);
  }, [terminalTheme, transcriptDefault, landingPage, terminalBehavior]);

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
    setAccountState({ status: "loading" });
    client.accountMe()
      .then((data) => setAccountState({ status: "account", data }))
      .catch((error: Error) => setAccountState({ status: "error", error: error.message }));
  }, [client, token]);

  if (view.name === "share") {
    return <PublicShareView linkId={view.linkId} shareToken={view.shareToken} />;
  }

  if (!token) {
    return (
      <AuthGate onAuthenticated={(value, remember) => {
        saveToken(value, remember);
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
        setToken("");
      }}
    >
      {view.name === "dashboard" && <DashboardView client={client} onOpen={openDetail} />}
      {view.name === "sessions" && <SessionsView client={client} onOpen={openDetail} />}
      {view.name === "origins" && <OriginsView client={client} />}
      {view.name === "terminals" && (
        <TerminalsView
          client={client}
          onOpenSession={navigateToTerminal}
        />
      )}
      {view.name === "profile" && (
        accountState.status === "account"
          ? <ProfileView client={client} account={accountState.data} onUserChange={updateAccountUser} />
          : <AccountRequiredPanel state={accountState} />
      )}
      {view.name === "settings" && (
        <SettingsView
          client={client}
          theme={theme}
          accent={accent}
          density={density}
          background={background}
          terminalTheme={terminalTheme}
          transcriptDefault={transcriptDefault}
          landingPage={landingPage}
          terminalBehavior={terminalBehavior}
          onThemeChange={setTheme}
          onAccentChange={setAccent}
          onDensityChange={setDensity}
          onBackgroundChange={setBackground}
          onTerminalThemeChange={setTerminalTheme}
          onTranscriptDefaultChange={setTranscriptDefault}
          onLandingPageChange={setLandingPage}
          onTerminalBehaviorChange={setTerminalBehavior}
        />
      )}
      {view.name === "detail" && (
        <DetailView
          client={client}
          accountToken={token}
          reference={view.ref}
          onOpenTerminal={openTerminalFromSession}
          transcriptDefault={transcriptDefault}
        />
      )}
      {view.name === "terminalLive" && (
        <Suspense fallback={<Loading title="Terminal Runtime" />}>
          <TerminalLiveView
            client={client}
            accountToken={token}
            reference={view.ref}
            terminalId={view.terminalId}
            terminalTheme={terminalTheme}
            onTerminalThemeChange={setTerminalTheme}
          />
        </Suspense>
      )}
    </Shell>
  );

  function openDetail(session: SessionSummary) {
    navigateToDetail({ origin: session.origin, provider: session.provider, id: session.id });
  }

  function navigateToDetail(ref: SessionRef) {
    window.location.hash = `session/${encodePart(ref.origin)}/${encodePart(ref.provider)}/${encodePart(ref.id)}`;
    setView({ name: "detail", ref });
  }

  function navigateToTerminal(ref: SessionRef, terminalId: string) {
    window.location.hash = terminalLiveHash(ref, terminalId);
    setView({ name: "terminalLive", ref, terminalId });
  }

  async function openTerminalFromSession(ref: SessionRef, mode: TerminalMode) {
    const terminal = await requestTerminalOpen(token, ref, mode);
    navigateToTerminal(terminal.session, terminal.terminal_id);
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
          <div>Runtime: daemon</div>
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

function AccountRequiredPanel({ state }: { state: AccountState }) {
  if (state.status === "loading") return <Loading title="Profile" />;
  if (state.status === "account") return null;
  return (
    <section className="center-panel error">
      <ShieldCheck size={18} />
      <h1>Account required</h1>
      <p>{state.error}</p>
    </section>
  );
}

function PublicShareView({ linkId, shareToken }: { linkId: string; shareToken: string }) {
  const client = useMemo(() => new ApiClient(""), []);
  const [state, setState] = useState<LoadState<SessionDetail>>({ status: "loading" });
  const [error, setError] = useState("");

  useEffect(() => {
    setState({ status: "loading" });
    setError("");
    client.publicShare(linkId, shareToken)
      .then((data) => setState({ status: "ready", data: data.session }))
      .catch((failure: Error) => {
        setState({ status: "error", error: failure.message });
        setError(failure.message);
      });
  }, [client, linkId, shareToken]);

  if (state.status === "loading" || state.status === "idle") {
    return <main className="public-share"><Loading title="Shared session" /></main>;
  }

  if (state.status === "error") {
    return (
      <main className="public-share">
        <section className="center-panel">
          <ShieldCheck size={18} />
          <h1>Shared view unavailable</h1>
          <p>Share unavailable.</p>
          <p className="small muted">{error}</p>
        </section>
      </main>
    );
  }

  const { summary, transcript } = state.data;
  const firstPrompt = summary.first_user_message?.trim() || "";
  const transcriptMessages = firstPrompt && transcript[0] && isFirstPromptDuplicate(firstPrompt, transcript[0]) ? transcript.slice(1) : transcript;

  return (
    <main className="public-share">
      <header className="public-share-head">
        <div className="brand-lockup">
          <div className="mark">c</div>
          <div><b>coca</b><span>read-only shared session</span></div>
        </div>
        <span className="status-badge info">public read-only</span>
      </header>
      <section className="transcript-shell" aria-label="Shared transcript">
        <header className="transcript-toolbar">
          <div><b>{summary.title}</b> <span className="small muted">{summary.provider} / {summary.origin}</span></div>
          <span className="tag">read-only</span>
        </header>
        <div className="timeline">
          {!firstPrompt && transcriptMessages.length === 0 && <EmptyState title="No transcript reconstructed" body="The shared session was found, but no ordered user or assistant text could be rebuilt." />}
          {firstPrompt && <FirstPromptItem prompt={firstPrompt} />}
          {transcriptMessages.map((message, index: number) => (
            <Message
              key={`${message.timestamp_ms ?? index}:${index}`}
              message={message}
              index={index + (firstPrompt ? 1 : 0)}
              defaultMode="rendered"
            />
          ))}
        </div>
      </section>
    </main>
  );
}

function DashboardView({ client, onOpen }: { client: ApiClient; onOpen: (session: SessionSummary) => void }) {
  const [sessions, setSessions] = useState<LoadState<SessionsResponse>>({ status: "loading" });
  const [config, setConfig] = useState<LoadState<ConfigSummary>>({ status: "loading" });
  const [terminals, setTerminals] = useState<LoadState<TerminalSessionsResponse>>({ status: "loading" });

  useEffect(() => {
    setSessions({ status: "loading" });
    client.sessions().then((data) => setSessions({ status: "ready", data })).catch((error: Error) => setSessions({ status: "error", error: error.message }));
    setConfig({ status: "loading" });
    client.configSummary().then((data) => setConfig({ status: "ready", data })).catch((error: Error) => setConfig({ status: "error", error: error.message }));
  }, [client]);

  useEffect(() => {
    setTerminals({ status: "loading" });
    client.terminalSessions().then((data) => setTerminals({ status: "ready", data })).catch((error: Error) => setTerminals({ status: "error", error: error.message }));
  }, [client]);

  const sessionData = sessions.status === "ready" ? sessions.data : null;
  const configData = config.status === "ready" ? config.data : null;
  const terminalData = terminals.status === "ready" ? terminals.data.terminals : [];
  const recentSessions = sessionData?.sessions.slice(0, 6) ?? [];
  const pinnedSessions = recentSessions.slice(0, 3);
  const activityRows = [
    ...terminalData.slice(0, 3).map((terminal) => ({
      id: `terminal:${terminal.terminal_id}`,
      label: terminal.terminal_id,
      detail: `${terminal.state.toLowerCase()} / ${terminal.session.provider} / ${terminal.session.origin}`
    })),
    ...recentSessions.slice(0, Math.max(0, 5 - Math.min(3, terminalData.length))).map((session) => ({
      id: `session:${sessionKey(session)}`,
      label: session.title,
      detail: `${session.provider} / ${session.origin} / ${session.updated_label}`
    }))
  ].slice(0, 5);

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
        {terminals.status === "loading" && <SkeletonRows count={4} />}
        {terminals.status === "error" && <Notice tone="error" title="Terminal registry unavailable" body={terminals.error} />}
        {terminalData.length === 0 && terminals.status === "ready" && <EmptyState title="No active terminals" body="The daemon registry returned no terminal sessions." />}
        <div className="list">
          {terminalData.slice(0, 5).map((terminal) => (
            <div className="list-row terminal-mini" key={terminal.terminal_id}>
              <span className="truncate"><b className="mono">{terminal.terminal_id}</b><span className="small muted">{terminal.session.provider} / {terminal.session.origin}</span></span>
              <StatusBadge label={terminal.state.toLowerCase()} tone={terminalTone(terminal.state)} />
              <a className="btn small-btn" href={terminalLiveHash(terminal.session, terminal.terminal_id)}>Attach</a>
            </div>
          ))}
        </div>
      </Module>
      <Module className="span-4" title="Pinned Sessions" icon={<ShieldCheck size={16} />}>
        {sessions.status === "loading" && <SkeletonRows count={3} />}
        {sessions.status === "error" && <Notice tone="error" title="Sessions unavailable" body={sessions.error} />}
        {pinnedSessions.length === 0 && sessions.status === "ready" && <EmptyState title="No pinned sessions" body="No sessions available." />}
        <div className="list">
          {pinnedSessions.map((session) => (
            <button className="list-row session-mini" type="button" key={sessionKey(session)} onClick={() => onOpen(session)}>
              <span className="truncate">{session.title}</span>
              <TerminalCapabilityBadge session={session} />
            </button>
          ))}
        </div>
      </Module>
      <Module className="span-4" title="Recent Activity" icon={<Activity size={16} />}>
        {(sessions.status === "loading" || terminals.status === "loading") && <SkeletonRows count={4} />}
        {sessions.status === "error" && <Notice tone="error" title="Sessions unavailable" body={sessions.error} />}
        {terminals.status === "error" && <Notice tone="error" title="Terminals unavailable" body={terminals.error} />}
        {activityRows.length === 0 && sessions.status === "ready" && terminals.status === "ready" && <EmptyState title="No activity" body="No sessions or terminals loaded." />}
        <div className="list">
          {activityRows.map((row) => (
            <div className="list-row activity-mini" key={row.id}>
              <span className="truncate"><b>{row.label}</b><span className="small muted truncate">{row.detail}</span></span>
            </div>
          ))}
        </div>
      </Module>
      <Module className="span-4" title="Provenance" icon={<Workflow size={16} />}>
        <div className="provenance-strip"><span>Browser</span><b>&gt;</b><span>Gateway</span><b>&gt;</b><span>Daemon</span></div>
        {config.status === "loading" && <SkeletonRows count={3} />}
        {config.status === "error" && <Notice tone="error" title="Config unavailable" body={config.error} />}
        {configData && (
          <dl className="meta-grid">
            <dt>Gateway bind</dt><dd className="mono truncate">{configData.gateway_bind}</dd>
            <dt>Share base</dt><dd className="mono truncate">{configData.share.base_url}</dd>
            <dt>Terminal</dt><dd>{configData.terminal.enabled ? "enabled" : "disabled"}</dd>
            <dt>Remotes</dt><dd>{configData.remotes.length}</dd>
          </dl>
        )}
      </Module>
    </div>
  );
}

function SessionsView({ client, onOpen }: { client: ApiClient; onOpen: (session: SessionSummary) => void }) {
  const [state, setState] = useState<LoadState<SessionsResponse>>({ status: "loading" });
  const [terminals, setTerminals] = useState<LoadState<TerminalSessionsResponse>>({ status: "loading" });
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

  useEffect(() => {
    setTerminals({ status: "loading" });
    client.terminalSessions()
      .then((data) => setTerminals({ status: "ready", data }))
      .catch((error: Error) => setTerminals({ status: "error", error: error.message }));
  }, [client]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Sessions" />;
  if (state.status === "error") return <ErrorPanel title="Sessions" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Sessions" />;

  const data = state.data;
  const providers = unique(data.sessions.map((session) => session.provider));
  const origins = unique(data.sessions.map((session) => session.origin));
  const models = unique(data.sessions.map((session) => session.model ?? "-"));
  const terminalIndex = terminals.status === "ready" ? terminalsBySession(terminals.data.terminals) : new Map<string, TerminalSessionSummary[]>();
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
      <div className="table-wrap sessions-table">
        <table>
          <colgroup>
            <col style={{ width: "42%" }} />
            <col style={{ width: "8%" }} />
            <col style={{ width: "7%" }} />
            <col style={{ width: "18%" }} />
            <col style={{ width: "10%" }} />
            <col style={{ width: "7%" }} />
            <col style={{ width: "4%" }} />
            <col style={{ width: "10%" }} />
          </colgroup>
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
                <td><SessionTerminalCell session={session} terminals={terminalIndex.get(sessionKey(session)) ?? []} /></td>
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
  accountToken,
  reference,
  onOpenTerminal,
  transcriptDefault
}: {
  client: ApiClient;
  accountToken: string;
  reference: SessionRef;
  onOpenTerminal: (ref: SessionRef, mode: TerminalMode) => Promise<void>;
  transcriptDefault: TranscriptDefault;
}) {
  const [state, setState] = useState<LoadState<SessionDetail>>({ status: "loading" });
  const [terminals, setTerminals] = useState<LoadState<TerminalSessionsResponse>>({ status: "loading" });
  const [share, setShare] = useState("");
  const [launchState, setLaunchState] = useState<{ status: "idle" | "connecting" | "error"; message: string }>({ status: "idle", message: "" });

  useEffect(() => {
    setState({ status: "loading" });
    setShare("");
    client.session(reference).then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client, reference.origin, reference.provider, reference.id]);

  useEffect(() => {
    setTerminals({ status: "loading" });
    client.terminalSessions()
      .then((data) => setTerminals({ status: "ready", data }))
      .catch((error: Error) => setTerminals({ status: "error", error: error.message }));
  }, [client]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Session" />;
  if (state.status === "error") return <ErrorPanel title="Session" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Session" />;

  const { summary, transcript } = state.data;
  const firstPrompt = summary.first_user_message?.trim() || "";
  const transcriptMessages = firstPrompt && transcript[0] && isFirstPromptDuplicate(firstPrompt, transcript[0]) ? transcript.slice(1) : transcript;
  const relatedTerminals = terminals.status === "ready"
    ? terminals.data.terminals.filter((terminal) => sameSessionRef(terminal.session, reference) && terminal.state !== "Exited")
    : [];
  const canResume = summary.terminal.enabled && summary.terminal.can_resume && Boolean(accountToken) && launchState.status !== "connecting";
  const canFork = summary.terminal.enabled && summary.terminal.can_fork && Boolean(accountToken) && launchState.status !== "connecting";

  async function launchTerminal(mode: TerminalMode) {
    setLaunchState({ status: "connecting", message: `Requesting ${mode.toLowerCase()} terminal...` });
    try {
      await onOpenTerminal(reference, mode);
      setLaunchState({ status: "idle", message: "" });
    } catch (error) {
      setLaunchState({ status: "error", message: error instanceof Error ? error.message : "Terminal open failed." });
    }
  }

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
      <div className="detail-layout">
        <section className="transcript-shell" aria-label="Transcript">
          <header className="transcript-toolbar">
            <div><b>Transcript Timeline</b></div>
            <span className="tag">sessions.read</span>
          </header>
          <div className="timeline">
            {!firstPrompt && transcriptMessages.length === 0 && <EmptyState title="No transcript reconstructed" body="The provider history was found, but no ordered user or assistant text could be rebuilt." />}
            {firstPrompt && <FirstPromptItem prompt={firstPrompt} />}
            {transcriptMessages.map((message, index: number) => (
              <Message
                key={`${message.timestamp_ms ?? index}:${index}`}
                message={message}
                index={index + (firstPrompt ? 1 : 0)}
                defaultMode={transcriptDefault}
              />
            ))}
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
              <dt>Scope</dt><dd>sessions.read</dd>
            </dl>
          </Module>
          <Module title="Runtime Readiness" icon={<TerminalSquare size={16} />} action={<TerminalCapabilityBadge session={summary} />}>
            {summary.terminal.unavailable_message && <Notice tone="warning" title={terminalReadiness(summary)} body={summary.terminal.unavailable_message} />}
            <div className="provenance-strip"><span>Browser</span><b>&gt;</b><span>Gateway</span><b>&gt;</b><span>Daemon</span></div>
            <div className="panel-actions runtime-actions">
              <button className="btn primary small-btn" type="button" disabled={!canResume} onClick={() => launchTerminal("Resume")}>Resume</button>
              <button className="btn small-btn" type="button" disabled={!canFork} onClick={() => launchTerminal("Fork")}>Fork</button>
            </div>
            {launchState.status !== "idle" && <p className={`save-status ${launchState.status === "error" ? "error-text" : ""}`}>{launchState.message}</p>}
          </Module>
          <Module title="Related Running Terminals" icon={<TerminalSquare size={16} />}>
            {terminals.status === "loading" && <p className="muted">Loading terminal registry...</p>}
            {terminals.status === "error" && <Notice tone="warning" title="Terminal registry unavailable" body={terminals.error} />}
            {terminals.status === "ready" && relatedTerminals.length === 0 && <p className="muted">No active terminals are registered for this session.</p>}
            {relatedTerminals.length > 0 && (
              <div className="terminal-link-list">
                {relatedTerminals.map((terminal) => <TerminalLinkRow terminal={terminal} key={terminal.terminal_id} />)}
              </div>
            )}
          </Module>
        </aside>
      </div>
    </div>
  );
}

function TerminalLiveView({
  client,
  accountToken,
  reference,
  terminalId,
  terminalTheme,
  onTerminalThemeChange
}: {
  client: ApiClient;
  accountToken: string;
  reference: SessionRef;
  terminalId: string;
  terminalTheme: TerminalTheme;
  onTerminalThemeChange: (theme: TerminalTheme) => void;
}) {
  const [state, setState] = useState<LoadState<SessionDetail>>({ status: "loading" });
  const [runtimeState, setRuntimeState] = useState("opening terminal stream");
  const [runtimeNotice, setRuntimeNotice] = useState<string | null>(null);

  useEffect(() => {
    setState({ status: "loading" });
    setRuntimeNotice(null);
    client.session(reference).then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client, reference.origin, reference.provider, reference.id]);

  if (state.status === "loading" || state.status === "idle") return <Loading title="Terminal Runtime" />;
  if (state.status === "error") return <ErrorPanel title="Terminal Runtime" error={state.error} />;
  if (state.status !== "ready") return <Loading title="Terminal Runtime" />;

  const { summary, transcript } = state.data;
  const transcriptHref = `#/session/${encodePart(reference.origin)}/${encodePart(reference.provider)}/${encodePart(reference.id)}`;
  const sourceTranscript = transcript.slice(0, 4);

  function handleTerminalOpened(terminal: TerminalSessionSummary) {
    const canonicalHash = terminalLiveHash(terminal.session, terminal.terminal_id);
    const currentHash = window.location.hash || "#/";
    if (terminal.terminal_id !== terminalId || !sameSessionRef(terminal.session, reference) || currentHash !== canonicalHash) {
      setRuntimeState("correcting terminal route");
      setRuntimeNotice(`Terminal registry returned ${terminal.terminal_id} for ${terminal.session.provider}/${terminal.session.origin}; correcting the live route.`);
      window.setTimeout(() => {
        window.location.hash = canonicalHash;
      }, 0);
      return;
    }
    setRuntimeState("attached");
    setRuntimeNotice(null);
  }

  return (
    <div className="terminal-workspace">
      <header className="terminal-live-header">
        <div className="terminal-live-title truncate">
          <span className="eyebrow">daemon runtime</span>
          <h2 title={summary.title}>{summary.title}</h2>
          <div className="badge-row">
            <ProviderBadge provider={summary.provider} />
            <OriginBadge origin={summary.origin} />
            <StatusBadge label={runtimeState} tone={runtimeState === "attached" ? "success" : runtimeState === "attach failed" ? "error" : "warning"} />
            <span className="tag mono">{terminalId}</span>
          </div>
        </div>
        <div className="terminal-live-actions">
          <a className="btn small-btn" href={transcriptHref}><FileText size={14} /> Transcript</a>
          <a className="btn small-btn" href="#/terminals"><TerminalSquare size={14} /> Registry</a>
        </div>
      </header>
      <div className="terminal-live-main">
        <TerminalPanel
          client={client}
          accountToken={accountToken}
          session={summary}
          reference={reference}
          initialAttachId={terminalId}
          variant="live"
          terminalTheme={terminalTheme}
          onAttachReady={handleTerminalOpened}
          onAttachError={() => setRuntimeState("attach failed")}
          onDetach={() => setRuntimeState("detached")}
        />
        <aside className="terminal-side" aria-label="Terminal context">
          {runtimeNotice && <Notice tone="warning" title="Terminal route corrected" body={runtimeNotice} />}
          <Module title="Runtime Boundary" icon={<Workflow size={16} />}>
            <div className="provenance-strip"><span>Browser</span><b>&gt;</b><span>Gateway</span><b>&gt;</b><span>Daemon</span></div>
          </Module>
          <Module title="Terminal Theme" icon={<TerminalSquare size={16} />}>
            <div className="theme-picker terminal-theme-picker">
              {terminalThemes.map((item) => (
                <button
                  className={`theme-choice ${terminalTheme === item.id ? "active" : ""}`}
                  style={{ "--swatch": item.swatch } as CSSProperties}
                  type="button"
                  key={item.id}
                  onClick={() => onTerminalThemeChange(item.id)}
                >
                  <span className="theme-swatch" />
                  <span>{item.label}</span>
                </button>
              ))}
            </div>
          </Module>
          <Module title="Terminal Metadata" icon={<Database size={16} />}>
            <dl className="meta-grid">
              <dt>Terminal</dt><dd className="mono truncate">{terminalId}</dd>
              <dt>Session id</dt><dd className="mono truncate">{summary.id}</dd>
              <dt>Provider</dt><dd>{summary.provider}</dd>
              <dt>Origin</dt><dd>{summary.origin}</dd>
              <dt>cwd</dt><dd className="mono truncate">{summary.cwd}</dd>
              <dt>Messages</dt><dd>{transcript.length}</dd>
            </dl>
          </Module>
          <Module title="Source Transcript" icon={<FileText size={16} />} action={<a className="btn small-btn" href={transcriptHref}>Open</a>}>
            <div className="list">
              {sourceTranscript.map((message, index) => (
                <a className="list-row session-mini" href={transcriptHref} key={`${message.timestamp_ms ?? index}:${index}`}>
                  <span className="truncate"><b className="mono">#{String(index + 1).padStart(2, "0")}</b> {excerpt(message.text)}</span>
                  <span className="tag">{transcriptRole(message.display_role)}</span>
                </a>
              ))}
              {sourceTranscript.length === 0 && <EmptyState title="No transcript" body="No messages loaded." />}
            </div>
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
        {remotes.length === 0 && <EmptyState title="No origins" body="No remotes match the current filters." />}
      </div>
    </div>
  );
}

function TerminalsView({
  client,
  onOpenSession
}: {
  client: ApiClient;
  onOpenSession: (ref: SessionRef, terminalId: string) => void;
}) {
  const [state, setState] = useState<LoadState<TerminalSessionsResponse>>({ status: "loading" });
  const [query, setQuery] = useState("");
  const [terminalState, setTerminalState] = useState("all");

  useEffect(() => {
    setState({ status: "loading" });
    client.terminalSessions().then((data) => setState({ status: "ready", data })).catch((error: Error) => setState({ status: "error", error: error.message }));
  }, [client]);

  const terminals = state.status === "ready" ? state.data.terminals.filter((terminal) => {
    const haystack = `${terminal.terminal_id} ${terminal.session.origin} ${terminal.session.provider} ${terminal.session.id} ${terminal.mode} ${terminal.state}`.toLowerCase();
    return (!query.trim() || haystack.includes(query.trim().toLowerCase())) &&
      (terminalState === "all" || terminal.state.toLowerCase() === terminalState);
  }) : [];

  return (
    <div className="stack wide-stack">
      <section className="toolbar">
        <div className="filter-row">
          <label className="filter-search"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search terminal id, session, origin" /></label>
          <Select value={terminalState} onChange={setTerminalState} options={["all", "starting", "running", "detached", "exited"]} label="State" />
        </div>
      </section>
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
                  <td><button className="btn primary small-btn" type="button" onClick={() => onOpenSession(terminal.session, terminal.terminal_id)} disabled={terminal.state === "Exited"}><Plug size={14} /> Attach</button></td>
                </tr>
              ))}
            </tbody>
          </table>
          {terminals.length === 0 && <EmptyState title="Filtered empty" body="No terminals match the current search or state filter." />}
        </div>
      )}
    </div>
  );
}

function SettingsView({
  client,
  theme,
  accent,
  density,
  background,
  terminalTheme,
  transcriptDefault,
  landingPage,
  terminalBehavior,
  onThemeChange,
  onAccentChange,
  onDensityChange,
  onBackgroundChange,
  onTerminalThemeChange,
  onTranscriptDefaultChange,
  onLandingPageChange,
  onTerminalBehaviorChange
}: {
  client: ApiClient;
  theme: Theme;
  accent: Accent;
  density: Density;
  background: Background;
  terminalTheme: TerminalTheme;
  transcriptDefault: TranscriptDefault;
  landingPage: LandingPage;
  terminalBehavior: TerminalBehavior;
  onThemeChange: (theme: Theme) => void;
  onAccentChange: (accent: Accent) => void;
  onDensityChange: (density: Density) => void;
  onBackgroundChange: (background: Background) => void;
  onTerminalThemeChange: (theme: TerminalTheme) => void;
  onTranscriptDefaultChange: (value: TranscriptDefault) => void;
  onLandingPageChange: (value: LandingPage) => void;
  onTerminalBehaviorChange: (value: TerminalBehavior) => void;
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
        <a href="#settings-access">Security / Access</a>
        <a href="#settings-devices">Device Sessions</a>
        <a href="#settings-runtime">Runtime</a>
        <a href="#settings-ai">AI</a>
        <a href="#settings-remotes">Remotes</a>
      </aside>
      <div className="settings-main">
        {data.warnings.length > 0 && <Warnings warnings={data.warnings} />}
        <Module id="settings-preferences" title="Preferences" icon={<Settings size={16} />} action={<span className="small muted">Local</span>}>
          <div className="setting-row"><div><b>Theme</b></div><Segmented value={theme} values={["light", "dark"]} onChange={(value) => onThemeChange(value as Theme)} icons={{ light: <Sun size={15} />, dark: <Moon size={15} /> }} /></div>
          <div className="setting-row"><div><b>Background</b></div><Segmented value={background} values={backgroundOptions.map((item) => item.id)} onChange={(value) => onBackgroundChange(value as Background)} /></div>
          <div className="setting-row"><div><b>Theme color</b></div><div className="theme-picker">{accentThemes.map((item) => <button className={`theme-choice ${accent === item.id ? "active" : ""}`} style={{ "--swatch": item.swatch } as CSSProperties} type="button" key={item.id} onClick={() => onAccentChange(item.id)}><span className="theme-swatch" /><span>{item.label}</span></button>)}</div></div>
          <div className="setting-row"><div><b>Terminal theme</b></div><div className="theme-picker">{terminalThemes.map((item) => <button className={`theme-choice ${terminalTheme === item.id ? "active" : ""}`} style={{ "--swatch": item.swatch } as CSSProperties} type="button" key={item.id} onClick={() => onTerminalThemeChange(item.id)}><span className="theme-swatch" /><span>{item.label}</span></button>)}</div></div>
          <div className="setting-row"><div><b>Density</b></div><Segmented value={density} values={["compact", "normal", "comfortable"]} onChange={(value) => onDensityChange(value as Density)} /></div>
          <div className="setting-row"><div><b>Transcript</b></div><Segmented value={transcriptDefault} values={["rendered", "raw"]} onChange={(value) => onTranscriptDefaultChange(value as TranscriptDefault)} /></div>
          <div className="setting-row"><div><b>Landing</b></div><Select value={landingPage} onChange={(value) => onLandingPageChange(value as LandingPage)} options={["dashboard", "sessions", "terminals"]} label="Page" /></div>
          <div className="setting-row"><div><b>Terminal behavior</b></div><Select value={terminalBehavior} onChange={(value) => onTerminalBehaviorChange(value as TerminalBehavior)} options={["detach-confirm-kill", "ask-before-detach"]} label="Close" /></div>
        </Module>
        <Module id="settings-access" title="Security / Access" icon={<ShieldCheck size={16} />} action={<a className="btn small-btn" href="#/profile">Profile</a>}>
          <div className="provenance-strip"><span>sessions.read</span><b>/</b><span>terminal.write</span><b>/</b><span>tokens.manage</span></div>
        </Module>
        <Module id="settings-devices" title="Device Sessions" icon={<UserRound size={16} />} action={<a className="btn small-btn" href="#/profile">Manage</a>}>
          <div className="meta-grid">
            <dt>Current account</dt><dd>Profile</dd>
            <dt>Session controls</dt><dd>Device list</dd>
          </div>
        </Module>
        <Module id="settings-runtime" title="Runtime" icon={<TerminalSquare size={16} />}>
          <div className="grid-two">
            <InfoPanel title="Service" rows={[
              ["active bind", data.bind],
              ["configured bind", data.gateway_bind],
              ["share base", data.share.base_url]
            ]} />
            <InfoPanel title="Terminal stream" rows={health.status === "ready" ? [
              ["protocol", health.data.stream.protocol],
              ["client events", health.data.stream.client_events.join(", ")],
              ["server events", health.data.stream.server_events.join(", ")]
            ] : [["status", "unavailable"]]} />
            <InfoPanel title="Terminal access" rows={[
              ["enabled", data.terminal.enabled ? "yes" : "no"],
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
          <span>Session summaries</span>
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

function Message({ message, index, defaultMode }: { message: SessionDetail["transcript"][number]; index: number; defaultMode: TranscriptDefault }) {
  const role = transcriptRole(message.display_role);
  const [expanded, setExpanded] = useState(() => !shouldCollapseMessage(message));
  const [mode, setMode] = useState<MessageMode>(defaultMode === "raw" ? "raw" : "preview");
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

function SessionTerminalCell({ session, terminals }: { session: SessionSummary; terminals: TerminalSessionSummary[] }) {
  const activeTerminals = terminals.filter((terminal) => terminal.state !== "Exited");
  if (activeTerminals.length === 0) return <TerminalCapabilityBadge session={session} />;
  return (
    <div className="session-terminal-cell">
      <TerminalCapabilityBadge session={session} />
      <div className="session-terminal-links">
        {activeTerminals.slice(0, 2).map((terminal) => (
          <a
            className="tag mono"
            href={terminalLiveHash(terminal.session, terminal.terminal_id)}
            key={terminal.terminal_id}
            onClick={(event) => event.stopPropagation()}
          >
            {terminal.terminal_id}
          </a>
        ))}
        {activeTerminals.length > 2 && <span className="tag">+{activeTerminals.length - 2}</span>}
      </div>
    </div>
  );
}

function TerminalLinkRow({ terminal }: { terminal: TerminalSessionSummary }) {
  return (
    <a className="terminal-link-row" href={terminalLiveHash(terminal.session, terminal.terminal_id)}>
      <span className="truncate">
        <b className="mono">{terminal.terminal_id}</b>
        <small>{terminal.mode.toLowerCase()} / {terminal.session.provider} / {terminal.session.origin}</small>
      </span>
      <StatusBadge label={terminal.state.toLowerCase()} tone={terminalTone(terminal.state)} />
    </a>
  );
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

function requestTerminalOpen(accountToken: string, ref: SessionRef, mode: TerminalMode): Promise<TerminalSessionSummary> {
  return new Promise((resolve, reject) => {
    if (!accountToken) {
      reject(new Error("Sign in before opening a terminal."));
      return;
    }

    let settled = false;
    const socket = openTerminalSocket(accountToken);

    function settleWithError(message: string) {
      if (settled) return;
      settled = true;
      try {
        socket.close();
      } catch {
        // The socket may already be closed by the browser.
      }
      reject(new Error(message));
    }

    socket.onopen = () => {
      sendTerminalFrame(socket, {
        event: "terminal.open",
        payload: { session: ref, mode, size: launcherTerminalSize }
      });
    };

    socket.onmessage = (event) => {
      let frame: TerminalServerFrame;
      try {
        frame = JSON.parse(event.data) as TerminalServerFrame;
      } catch (error) {
        settleWithError(error instanceof Error ? error.message : "Invalid terminal frame.");
        return;
      }
      if (frame.event === "terminal.opened") {
        settled = true;
        socket.close();
        resolve(frame.payload.terminal);
        return;
      }
      if (frame.event === "terminal.error") {
        settleWithError(formatTerminalError(frame.payload));
      }
    };

    socket.onerror = () => settleWithError("Terminal stream connection failed.");
    socket.onclose = () => {
      if (!settled) settleWithError("Terminal stream closed before terminal.opened.");
    };
  });
}

function formatTerminalError(error: { message: string; action?: string | null; detail?: string | null }) {
  const parts = [error.message, error.action, error.detail].map((part) => part?.trim()).filter(Boolean);
  return parts.join(" ");
}

function unique(values: string[]) {
  return Array.from(new Set(values)).sort((left, right) => left.localeCompare(right));
}

function sessionKey(session: SessionRef) {
  return `${session.origin}:${session.provider}:${session.id}`;
}

function terminalsBySession(terminals: TerminalSessionSummary[]) {
  const index = new Map<string, TerminalSessionSummary[]>();
  for (const terminal of terminals) {
    if (terminal.state === "Exited") continue;
    const key = sessionKey(terminal.session);
    const entries = index.get(key) ?? [];
    entries.push(terminal);
    index.set(key, entries);
  }
  return index;
}

function sameSessionRef(left: SessionRef, right: SessionRef) {
  return left.origin === right.origin && left.provider === right.provider && left.id === right.id;
}

function terminalLiveHash(ref: SessionRef, terminalId: string) {
  return `#/terminal/${encodePart(ref.origin)}/${encodePart(ref.provider)}/${encodePart(ref.id)}/${encodePart(terminalId)}`;
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
      return { title: "Dashboard", subtitle: "Operational overview" };
    case "sessions":
      return { title: "Sessions", subtitle: "Browse normalized agent history" };
    case "origins":
      return { title: "Origins", subtitle: "Remote inventory" };
    case "terminals":
      return { title: "Active Terminals", subtitle: "Runtime registry" };
    case "terminalLive":
      return { title: "Terminal Runtime", subtitle: "Live daemon stream" };
    case "share":
      return { title: "Shared Session", subtitle: "Public read-only transcript" };
    case "profile":
      return { title: "Profile", subtitle: "Identity and access" };
    case "settings":
      return { title: "Settings", subtitle: "Preferences and runtime" };
    case "detail":
      return { title: "Session Detail", subtitle: "Transcript and runtime" };
  }
}

function isNavActive(view: View, name: View["name"]) {
  if (name === "sessions" && view.name === "detail") return true;
  if (name === "terminals" && view.name === "terminalLive") return true;
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
  if (accountState.status === "error") {
    return { initials: "cx", name: "Account unavailable", detail: accountState.error };
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
  if (!hash) return { name: readLandingPage() === "sessions" ? "sessions" : readLandingPage() === "terminals" ? "terminals" : "dashboard" };
  if (hash === "dashboard") return { name: "dashboard" };
  if (hash === "sessions") return { name: "sessions" };
  if (hash === "origins") return { name: "origins" };
  if (hash === "terminals") return { name: "terminals" };
  if (hash === "profile") return { name: "profile" };
  if (hash === "settings" || hash === "config") return { name: "settings" };
  const parts = hash.split("/");
  if (parts[0] === "share" && parts.length === 2) {
    const params = new URLSearchParams(query);
    return {
      name: "share",
      linkId: decodeURIComponent(parts[1]),
      shareToken: params.get("share_token") ?? ""
    };
  }
  if (parts[0] === "terminal" && parts.length === 5) {
    return {
      name: "terminalLive",
      ref: {
        origin: decodeURIComponent(parts[1]),
        provider: decodeURIComponent(parts[2]),
        id: decodeURIComponent(parts[3])
      },
      terminalId: decodeURIComponent(parts[4])
    };
  }
  if (parts[0] === "session" && parts.length === 4) {
    const params = new URLSearchParams(query);
    const terminalId = params.get("terminal");
    const ref = {
      origin: decodeURIComponent(parts[1]),
      provider: decodeURIComponent(parts[2]),
      id: decodeURIComponent(parts[3])
    };
    if (terminalId) {
      return { name: "terminalLive", ref, terminalId };
    }
    return {
      name: "detail",
      ref
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

function readBackground(): Background {
  const stored = window.localStorage.getItem("coca-web-background");
  return backgroundOptions.some((item) => item.id === stored) ? stored as Background : "porcelain";
}

function readTerminalTheme(): TerminalTheme {
  const stored = window.localStorage.getItem("coca-web-terminal-theme");
  return terminalThemes.some((item) => item.id === stored) ? stored as TerminalTheme : "one-half-dark";
}

function readTranscriptDefault(): TranscriptDefault {
  const stored = window.localStorage.getItem("coca-web-transcript-default");
  return stored === "raw" ? "raw" : "rendered";
}

function readLandingPage(): LandingPage {
  const stored = window.localStorage.getItem("coca-web-landing-page");
  if (stored === "sessions" || stored === "terminals") return stored;
  return "dashboard";
}

function readTerminalBehavior(): TerminalBehavior {
  const stored = window.localStorage.getItem("coca-web-terminal-behavior");
  return stored === "ask-before-detach" ? "ask-before-detach" : "detach-confirm-kill";
}

function encodePart(value: string): string {
  return encodeURIComponent(value);
}

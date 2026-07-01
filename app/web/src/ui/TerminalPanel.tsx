import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { GitFork, Maximize2, Minimize2, Plug, RefreshCw, RotateCw, Square, TerminalSquare, Unplug } from "lucide-react";
import {
  ApiClient,
  ApiError,
  openTerminalSocket,
  sendTerminalFrame
} from "../api/client";
import type {
  SessionRef,
  SessionSummary,
  TerminalClientFrame,
  TerminalMode,
  TerminalServerFrame,
  TerminalSessionSummary,
  TerminalSessionsResponse,
  TerminalSize
} from "../api/types";

type LoadState<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; data: T }
  | { status: "error"; error: DisplayError };

type ConnectionStatus = "idle" | "connecting" | "open" | "closed" | "error";
type TerminalTheme = "one-half-dark" | "campbell" | "vintage" | "solarized-dark" | "tango-dark";

type DisplayError = {
  code: string;
  message: string;
  action: string | null;
  detail: string | null;
};

const terminalRecentKey = "coca-terminal-recent";
const defaultSize: TerminalSize = { cols: 80, rows: 24 };

export function TerminalPanel({
  client,
  accountToken,
  session,
  reference,
  initialAttachId = null,
  variant = "detail",
  terminalTheme = "one-half-dark",
  onOpenTerminal,
  onAttachReady,
  onAttachError,
  onDetach
}: {
  client: ApiClient;
  accountToken: string;
  session: SessionSummary;
  reference: SessionRef;
  initialAttachId?: string | null;
  variant?: "detail" | "live";
  terminalTheme?: TerminalTheme;
  onOpenTerminal?: (mode: TerminalMode) => Promise<void> | void;
  onAttachReady?: (terminal: TerminalSessionSummary) => void;
  onAttachError?: (message: string) => void;
  onDetach?: () => void;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const activeTerminalIdRef = useRef<string | null>(null);
  const decoderRef = useRef(new TextDecoder());
  const lastSeqRef = useRef<number | null>(null);
  const lastSizeRef = useRef<TerminalSize>(defaultSize);
  const [listState, setListState] = useState<LoadState<TerminalSessionsResponse>>({ status: "loading" });
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>("idle");
  const [statusMessage, setStatusMessage] = useState("No terminal attached.");
  const [activeTerminalId, setActiveTerminalId] = useState<string | null>(null);
  const [activeSummary, setActiveSummary] = useState<TerminalSessionSummary | null>(null);
  const [openingMode, setOpeningMode] = useState<TerminalMode | null>(null);
  const [maximized, setMaximized] = useState(false);
  const initialAttachConsumedRef = useRef<string | null>(null);

  const canResume = session.terminal.enabled && session.terminal.can_resume;
  const canFork = session.terminal.enabled && session.terminal.can_fork;
  const unavailableMessage = session.terminal.unavailable_message ?? "Terminal access is unavailable for this session.";
  const isLive = variant === "live";

  useEffect(() => {
    activeTerminalIdRef.current = activeTerminalId;
  }, [activeTerminalId]);

  const refreshTerminals = useCallback(() => {
    setListState({ status: "loading" });
    client
      .terminalSessions()
      .then((data) => {
        setListState({ status: "ready", data });
        rememberTerminals(data.terminals);
      })
      .catch((error: Error) => setListState({ status: "error", error: displayError(error) }));
  }, [client]);

  useEffect(() => {
    refreshTerminals();
  }, [refreshTerminals]);

  const fitAndResize = useCallback(() => {
    const terminal = terminalRef.current;
    const fit = fitRef.current;
    if (!terminal || !fit) return defaultSize;
    fit.fit();
    const size = { cols: terminal.cols || defaultSize.cols, rows: terminal.rows || defaultSize.rows };
    const previous = lastSizeRef.current;
    lastSizeRef.current = size;
    const socket = socketRef.current;
    const terminalId = activeTerminalIdRef.current;
    if (
      terminalId &&
      socket?.readyState === WebSocket.OPEN &&
      (previous.cols !== size.cols || previous.rows !== size.rows)
    ) {
      sendTerminalFrame(socket, {
        event: "terminal.resize",
        payload: { terminal_id: terminalId, size }
      });
    }
    return size;
  }, []);

  const currentSize = useCallback(() => {
    const terminal = terminalRef.current;
    if (!terminal) return defaultSize;
    return { cols: terminal.cols || defaultSize.cols, rows: terminal.rows || defaultSize.rows };
  }, []);

  const closeSocket = useCallback(() => {
    const socket = socketRef.current;
    socketRef.current = null;
    if (socket && socket.readyState !== WebSocket.CLOSED && socket.readyState !== WebSocket.CLOSING) {
      socket.close();
    }
  }, []);

  const handleServerFrame = useCallback((frame: TerminalServerFrame) => {
    if (frame.event === "terminal.opened") {
      const summary = frame.payload.terminal;
      setActiveTerminalId(summary.terminal_id);
      setActiveSummary(summary);
      setStatusMessage(`${summary.state.toLowerCase()} terminal ${summary.terminal_id}`);
      lastSeqRef.current = summary.last_seq;
      rememberTerminal(summary);
      onAttachReady?.(summary);
      return;
    }
    if (frame.event === "terminal.output") {
      if (activeTerminalIdRef.current && frame.payload.terminal_id !== activeTerminalIdRef.current) return;
      lastSeqRef.current = frame.payload.seq;
      terminalRef.current?.write(decoderRef.current.decode(base64ToBytes(frame.payload.data_b64), { stream: true }));
      return;
    }
    if (frame.event === "terminal.exit") {
      const { terminal_id, exit } = frame.payload;
      setStatusMessage(`terminal ${terminal_id} exited${exit.code === null ? "" : ` with code ${exit.code}`}`);
      setActiveSummary((current) => current && current.terminal_id === terminal_id ? { ...current, state: "Exited", exit } : current);
      terminalRef.current?.writeln("");
      terminalRef.current?.writeln(`[coca] terminal exited${exit.code === null ? "" : ` with code ${exit.code}`}`);
      refreshTerminals();
      return;
    }
    const error = frame.payload;
    const message = formatDisplayError(error);
    setConnectionStatus("error");
    setStatusMessage(message);
    onAttachError?.(message);
    terminalRef.current?.writeln("");
    terminalRef.current?.writeln(`[coca] ${message}`);
    if (error.detail) terminalRef.current?.writeln(`[coca] detail: ${error.detail}`);
  }, [onAttachError, onAttachReady, refreshTerminals]);

  const connectWithFrame = useCallback((frame: TerminalClientFrame, clear = false) => {
    if (!accountToken) {
      setStatusMessage("Sign in before opening a terminal.");
      return;
    }
    closeSocket();
    decoderRef.current = new TextDecoder();
    const terminal = terminalRef.current;
    if (clear) terminal?.clear();
    terminal?.focus();
    const socket = openTerminalSocket(accountToken);
    socketRef.current = socket;
    setConnectionStatus("connecting");
    setStatusMessage("Connecting terminal stream...");
    socket.onopen = () => {
      setConnectionStatus("open");
      sendTerminalFrame(socket, frame);
      window.setTimeout(fitAndResize, 0);
    };
    socket.onmessage = (event) => {
      try {
        handleServerFrame(JSON.parse(event.data) as TerminalServerFrame);
      } catch (error) {
        const message = error instanceof Error ? error.message : "invalid terminal frame";
        setConnectionStatus("error");
        setStatusMessage(message);
      }
    };
    socket.onerror = () => {
      setConnectionStatus("error");
      setStatusMessage("Terminal stream connection failed.");
    };
    socket.onclose = () => {
      socketRef.current = null;
      setConnectionStatus((current) => current === "error" ? current : "closed");
    };
  }, [accountToken, closeSocket, fitAndResize, handleServerFrame]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const terminal = new Terminal({
      cursorBlink: true,
      fontFamily: "IBM Plex Mono, SFMono-Regular, Consolas, monospace",
      fontSize: 13,
      convertEol: true,
      scrollback: 5000,
      theme: terminalPalette(terminalTheme)
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    terminalRef.current = terminal;
    fitRef.current = fit;
    const dataSubscription = terminal.onData((data) => {
      const socket = socketRef.current;
      const terminalId = activeTerminalIdRef.current;
      if (!terminalId || socket?.readyState !== WebSocket.OPEN) return;
      sendTerminalFrame(socket, {
        event: "terminal.input",
        payload: { terminal_id: terminalId, data_b64: bytesToBase64(new TextEncoder().encode(data)) }
      });
    });
    const resizeObserver = new ResizeObserver(() => window.setTimeout(fitAndResize, 0));
    resizeObserver.observe(host);
    window.setTimeout(fitAndResize, 0);
    return () => {
      resizeObserver.disconnect();
      dataSubscription.dispose();
      closeSocket();
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
    };
  }, [closeSocket, fitAndResize, terminalTheme]);

  const listedTerminals = listState.status === "ready" ? listState.data.terminals : [];
  const relatedRegistryTerminals = listedTerminals.filter((terminal) => sameSession(terminal.session, reference) && terminal.state !== "Exited");
  const canOpenTerminal = Boolean(accountToken);

  async function openTerminal(mode: TerminalMode) {
    if (!isLive && onOpenTerminal) {
      setConnectionStatus("connecting");
      setOpeningMode(mode);
      setStatusMessage(`Opening ${mode.toLowerCase()} terminal...`);
      try {
        await onOpenTerminal(mode);
        setStatusMessage("Terminal opened. Routing to live terminal...");
      } catch (error) {
        const message = error instanceof Error ? error.message : "Terminal open failed.";
        setConnectionStatus("error");
        setStatusMessage(message);
        onAttachError?.(message);
      } finally {
        setOpeningMode(null);
      }
      return;
    }
    connectWithFrame({
      event: "terminal.open",
      payload: { session: reference, mode, size: currentSize() }
    }, true);
  }

  function attachTerminal(terminalId: string) {
    const id = terminalId.trim();
    if (!id) return;
    connectWithFrame({
      event: "terminal.attach",
      payload: { terminal_id: id, since_seq: null, size: currentSize() }
    });
  }

  useEffect(() => {
    const id = initialAttachId?.trim();
    if (!id || !accountToken || initialAttachConsumedRef.current === id) return;
    initialAttachConsumedRef.current = id;
    attachTerminal(id);
  }, [initialAttachId, accountToken]);

  function detachTerminal() {
    const terminalId = activeTerminalIdRef.current;
    const socket = socketRef.current;
    if (terminalId && socket?.readyState === WebSocket.OPEN) {
      sendTerminalFrame(socket, { event: "terminal.detach", payload: { terminal_id: terminalId } });
    }
    closeSocket();
    setActiveSummary((current) => current ? { ...current, state: "Detached", attached_clients: Math.max(0, current.attached_clients - 1) } : current);
    setActiveTerminalId(null);
    setStatusMessage(terminalId ? `detached from ${terminalId}` : "No terminal attached.");
    refreshTerminals();
    onDetach?.();
  }

  function killTerminal() {
    const terminalId = activeTerminalIdRef.current;
    const socket = socketRef.current;
    if (!terminalId || socket?.readyState !== WebSocket.OPEN) return;
    const confirmed = window.confirm(
      `Kill terminal ${terminalId}? This terminates the provider process. Use Detach to keep it running.`
    );
    if (!confirmed) return;
    sendTerminalFrame(socket, { event: "terminal.close", payload: { terminal_id: terminalId, kill: true } });
    setStatusMessage(`kill requested for ${terminalId}`);
  }

  return (
    <section className={`terminal-panel ${isLive ? "live" : "detail"} ${maximized ? "maximized" : ""}`} aria-label="Terminal">
      <header className="section-head terminal-head">
        <div>
          <p>terminal</p>
          <h2>{isLive ? "Terminal Runtime" : "Runtime session"}</h2>
        </div>
        <div className="terminal-status-strip">
          <span className={`terminal-state ${connectionStatus}`}>{connectionStatus}</span>
          {activeSummary && <span className="terminal-state">{activeSummary.state.toLowerCase()}</span>}
          {isLive && (
            <button className="icon-button" type="button" onClick={() => setMaximized((value) => !value)} aria-label={maximized ? "Restore terminal" : "Maximize terminal"}>
              {maximized ? <Minimize2 size={15} /> : <Maximize2 size={15} />}
            </button>
          )}
        </div>
      </header>

      {!session.terminal.enabled && (
        <div className="notice terminal-notice">
          <strong>{readinessLabel(session.terminal.unavailable_code)}</strong>
          <span>{unavailableMessage}</span>
        </div>
      )}

      <div className="terminal-action-grid">
        {!isLive && (
          <>
            <button className="icon-line" type="button" disabled={!canResume || !canOpenTerminal || openingMode !== null} title={!canResume ? unavailableMessage : undefined} onClick={() => openTerminal("Resume")}>
              <RotateCw size={16} />Resume
            </button>
            <button className="icon-line" type="button" disabled={!canFork || !canOpenTerminal || openingMode !== null} title={!canFork ? unavailableMessage : undefined} onClick={() => openTerminal("Fork")}>
              <GitFork size={16} />Fork
            </button>
          </>
        )}
        {isLive && (
          <>
            <button className="icon-line secondary" type="button" disabled={!activeTerminalId} onClick={detachTerminal}>
              <Unplug size={16} />Detach
            </button>
            <button className="icon-line danger" type="button" disabled={!activeTerminalId || connectionStatus !== "open"} onClick={killTerminal}>
              <Square size={16} />Kill
            </button>
          </>
        )}
      </div>

      {!isLive && (
        <div className="terminal-detail-body">
          <div className="terminal-toolbar detail-toolbar">
            <span><TerminalSquare size={15} />{openingMode ? `${openingMode.toLowerCase()} requested` : "runtime actions"}</span>
            <span>{statusMessage}</span>
          </div>
          <div className="terminal-sidebar">
            <div className="terminal-list-head">
              <strong>Related running terminals</strong>
              <button className="icon-button" type="button" onClick={refreshTerminals} aria-label="Refresh terminal sessions">
                <RefreshCw size={15} />
              </button>
            </div>
            {listState.status === "error" && (
              <TerminalErrorMessage error={listState.error} />
            )}
            {listState.status === "loading" && <p className="terminal-list-message">Loading terminal sessions...</p>}
            <TerminalList title="This session" terminals={relatedRegistryTerminals} activeTerminalId={null} linkMode />
          </div>
        </div>
      )}

      {isLive && (
        <div className="terminal-layout">
          <div className="terminal-surface-wrap">
            <div className="terminal-toolbar">
              <span><TerminalSquare size={15} />{activeTerminalId ?? "no terminal"}</span>
              <span>{statusMessage}</span>
            </div>
            <div className="terminal-surface" ref={hostRef} />
          </div>
        </div>
      )}
    </section>
  );
}

function TerminalList({
  title,
  terminals,
  activeTerminalId,
  onAttach,
  linkMode = false
}: {
  title: string;
  terminals: TerminalSessionSummary[];
  activeTerminalId: string | null;
  onAttach?: (terminalId: string) => void;
  linkMode?: boolean;
}) {
  return (
    <div className="terminal-list">
      <h3>{title}</h3>
      {terminals.length === 0 && <p className="terminal-list-message">None.</p>}
      {terminals.map((terminal) => (
        <div className="terminal-list-row" key={terminal.terminal_id}>
          <div>
            <strong>{terminal.terminal_id}</strong>
            <span>{terminal.mode.toLowerCase()} / {terminal.state.toLowerCase()} / {terminal.session.origin}</span>
          </div>
          {linkMode ? (
            <a className="icon-button" href={terminalLiveHash(terminal.session, terminal.terminal_id)} aria-label={`Open ${terminal.terminal_id}`}>
              <Plug size={15} />
            </a>
          ) : (
            <button className="icon-button" type="button" onClick={() => onAttach?.(terminal.terminal_id)} disabled={activeTerminalId === terminal.terminal_id || terminal.state === "Exited"} aria-label={`Attach ${terminal.terminal_id}`}>
              <Plug size={15} />
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

function TerminalErrorMessage({ error }: { error: DisplayError }) {
  return (
    <div className="terminal-list-message terminal-error-message">
      <strong>{error.message}</strong>
      {error.action && <span>{error.action}</span>}
      {error.detail && <small>{error.detail}</small>}
    </div>
  );
}

function displayError(error: Error): DisplayError {
  if (error instanceof ApiError) {
    return {
      code: error.code,
      message: error.message,
      action: error.action,
      detail: error.detail
    };
  }
  return {
    code: "unknown",
    message: error.message || "Terminal request failed.",
    action: "Refresh the terminal list or retry the action.",
    detail: null
  };
}

function formatDisplayError(error: { code: string; message: string; action?: string | null }) {
  const action = error.action?.trim();
  return action ? `${error.message} ${action}` : error.message;
}

function readinessLabel(code: string | null) {
  switch (code) {
    case "terminal_disabled":
      return "Terminal disabled";
    case "provider_cli_missing":
      return "Provider CLI missing";
    case "daemon_unavailable":
      return "Daemon unavailable";
    case "terminal_socket_unavailable":
      return "Terminal socket unavailable";
    case "remote_browse_only":
      return "Browse-only remote";
    case "remote_auth_failed":
      return "Remote auth required";
    case "remote_gateway_unreachable":
      return "Remote gateway unreachable";
    case "unsupported_platform":
      return "Unsupported platform";
    default:
      return "Terminal unavailable";
  }
}

function terminalPalette(theme: TerminalTheme) {
  switch (theme) {
    case "campbell":
      return { background: "#0c0c0c", foreground: "#cccccc", cursor: "#ffffff", selectionBackground: "#264f78" };
    case "vintage":
      return { background: "#000000", foreground: "#c0c0c0", cursor: "#00ff00", selectionBackground: "#333333" };
    case "solarized-dark":
      return { background: "#002b36", foreground: "#839496", cursor: "#93a1a1", selectionBackground: "#073642" };
    case "tango-dark":
      return { background: "#171a16", foreground: "#d3d7cf", cursor: "#eeeeec", selectionBackground: "#3465a4" };
    case "one-half-dark":
    default:
      return { background: "#282c34", foreground: "#dcdfe4", cursor: "#61afef", selectionBackground: "#3e4451" };
  }
}

function sameSession(left: SessionRef, right: SessionRef) {
  return left.origin === right.origin && left.provider === right.provider && left.id === right.id;
}

function terminalLiveHash(ref: SessionRef, terminalId: string) {
  return `#/terminal/${encodeURIComponent(ref.origin)}/${encodeURIComponent(ref.provider)}/${encodeURIComponent(ref.id)}/${encodeURIComponent(terminalId)}`;
}

function mergeTerminals(primary: TerminalSessionSummary[], secondary: TerminalSessionSummary[]) {
  const seen = new Set<string>();
  const merged: TerminalSessionSummary[] = [];
  for (const terminal of [...primary, ...secondary]) {
    if (seen.has(terminal.terminal_id)) continue;
    seen.add(terminal.terminal_id);
    merged.push(terminal);
  }
  return merged;
}

function rememberTerminal(terminal: TerminalSessionSummary) {
  rememberTerminals([terminal]);
}

function rememberTerminals(terminals: TerminalSessionSummary[]) {
  if (terminals.length === 0) return;
  const merged = mergeTerminals(terminals, readRecentTerminals()).slice(0, 16);
  window.localStorage.setItem(terminalRecentKey, JSON.stringify(merged));
}

function readRecentTerminals(): TerminalSessionSummary[] {
  const raw = window.localStorage.getItem(terminalRecentKey);
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as TerminalSessionSummary[];
    return Array.isArray(parsed) ? parsed.filter(isTerminalSummary) : [];
  } catch {
    return [];
  }
}

function isTerminalSummary(value: TerminalSessionSummary): value is TerminalSessionSummary {
  return Boolean(value?.terminal_id && value.session?.origin && value.session?.provider && value.session?.id);
}

function bytesToBase64(bytes: Uint8Array) {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return window.btoa(binary);
}

function base64ToBytes(data: string) {
  const binary = window.atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

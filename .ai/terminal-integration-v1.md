# Terminal Integration v1 Plan

## Goal

Add a unified terminal integration so users can continue or fork any local or
configured remote coder-agent session from Web now, and from TUI/GUI later,
without needing to reason about whether the terminal process is local or remote.

The product journey is complete when this works reliably:

```text
Find a historical session
  -> understand whether it can be continued and why
  -> Resume or Fork into a real terminal
  -> refresh/close the frontend without killing the process
  -> reattach later from coca
  -> detach or explicitly kill the terminal session
```

Provider history files remain read-only. Terminal runtime state is separate
from historical session data and is owned by `coca daemon`.

## Current Baseline

The current worktree already contains a terminal integration vertical slice:

- Terminal settings, generated terminal token, remote `terminal_token`, and
  redacted summaries.
- Terminal protocol envelopes and daemon terminal list/get RPC methods.
- Daemon-owned `TerminalManager`, `TerminalSession`, attach/detach, active
  writer, scrollback, output sequence, kill, and replay behavior.
- A `portable-pty` local backend for provider processes.
- A daemon terminal stream socket.
- A Web terminal WebSocket gateway that checks read token plus terminal token
  and forwards frames to daemon.
- A remote terminal proxy path from local daemon to remote gateway to remote
  daemon.
- A Web xterm-based panel with Resume, Fork, Attach, Detach, Kill, resize,
  terminal list, and separate terminal token storage.

Known remaining product gaps are UX clarity, structured readiness/error states,
browser refresh behavior, and architecture vocabulary/process-boundary
alignment.

## 1.1 User Experience Completion Plan

V1 is Web-first. TUI and GUI must remain architectural consumers of the same
daemon API, but a full TUI terminal emulator is out of scope for this release.

Terminal must be presented as a first-class runtime object:

- `Session`: read-only provider history loaded from local or remote catalogs.
- `TerminalSession`: daemon runtime state for one resumed/forked process,
  including lifecycle state, clients, active writer, scrollback, and output
  replay.

Required user-visible states:

- `Starting`: process/PTY is being created.
- `Running`: process is alive and at least one client may be attached.
- `Detached`: process is alive but no frontend is currently attached.
- `Exited`: provider process ended.
- `Failed`: terminal could not be opened or attached.

Required actions:

- `Resume`: continue the historical provider session.
- `Fork`: start a new provider session from the selected history context.
- `Attach`: connect to an existing daemon-owned `TerminalSession`.
- `Detach`: close only the frontend connection; keep the provider process
  running.
- `Kill`: terminate the `TerminalSession` and provider child process; require
  explicit confirmation.

Readiness must be visible before launch. Session list/detail should expose a
stable terminal readiness state and actionable reason:

- `ready`
- `terminal_disabled`
- `missing_terminal_token`
- `daemon_unavailable`
- `terminal_socket_unavailable`
- `provider_cli_missing`
- `remote_browse_only`
- `remote_gateway_unreachable`
- `remote_auth_failed`
- `unsupported_platform`

Refresh and reattach behavior:

- Browser refresh must not auto-attach to a terminal.
- `terminal_id` must not be encoded in the URL.
- The daemon terminal list is the source of truth for reattachment.
- The Web UI shows running/detached/exited terminal sessions after refresh.
- The user explicitly chooses `Attach`.
- Initial attach or refresh attach uses `since_seq: null` to replay available
  scrollback before live output resumes.
- Later incremental attach may use a known `since_seq` when the client has one.

Error model:

- Terminal HTTP and WebSocket setup failures should map to a structured error:

```json
{
  "code": "daemon_unavailable",
  "message": "Terminal daemon is not available.",
  "action": "Start coca daemon and retry.",
  "detail": "optional diagnostic text"
}
```

- UI text should display only actionable user-facing information by default.
- Internal Rust errors, socket paths, and stack-like diagnostics belong in
  collapsible detail/logging, not primary UI copy.
- Read/share auth failures and terminal auth failures must be visually distinct.

Security and sharing:

- `share.token` remains read-only.
- `terminal.token` gates write-capable terminal access.
- Browser JavaScript never receives remote terminal tokens.
- Shared/read-only pages must not show write-capable terminal controls.

Deferred UX features:

- Daemon crash/restart recovery for existing PTYs.
- Multiple active writers.
- tmux-style panes, windows, layouts, and custom naming UI.
- Perfect alternate-screen restoration through a headless terminal model.
- Per-open launch option editing.
- Advanced browser shortcut configuration.
- Search/indexing across captured terminal output.

## 1.2 Architecture Alignment

The architecture target is defined in
`.ai/runtime-process-architecture-plan.md`. The important terminal-specific
constraints are:

- `coca daemon` owns terminal lifecycle, PTY/PTS handles, child processes,
  attach/detach state, active-writer state, scrollback, and output replay.
- `coca gateway` is a browser HTTP/WebSocket edge only.
- Web/TUI/GUI are frontend/view processes and must not own terminal lifecycle.
- `coca-app` defines user-facing workflows, DTOs, settings orchestration, and
  launch resolution.
- `coca-core` keeps provider/session/launch primitives and remains
  process-agnostic.
- Remote terminal-capable machines run a remote gateway for network access and
  a remote daemon for PTY/process ownership.

Target process view:

```text
Browser / app/web
  |
  | HTTP / WebSocket
  v
coca gateway
  |
  | local IPC / terminal stream
  v
coca daemon
  |
  | TerminalManager + PTY
  v
provider CLI
```

Remote process view:

```text
local Browser/TUI
  |
  v
local gateway or frontend
  |
  v
local daemon
  |
  | remote terminal client
  v
remote gateway
  |
  v
remote daemon
  |
  v
remote PTY + provider CLI
```

## Public Interfaces

Settings:

```json
{
  "terminal": {
    "enabled": false,
    "token": "<generated>"
  },
  "remotes": [
    {
      "name": "work-mac",
      "base_url": "http://192.168.1.20:8787",
      "token": "<read-token>",
      "terminal_token": "<terminal-token>",
      "enabled": true
    }
  ]
}
```

Config summaries should expose capability state only:

- `terminal.enabled`
- `terminal.token_configured`
- `terminal.daemon_available`
- `terminal.terminal_socket_available`
- `terminal.unavailable_code`
- `terminal.unavailable_message`
- remote `terminal_token_configured`
- remote terminal readiness and unavailable reason

Web terminal endpoint:

```text
/api/v1/terminal/ws?token=<read-token>&terminal_token=<terminal-token>
```

Daemon terminal stream events:

```text
client -> server
terminal.open   { session, mode, cols, rows }
terminal.attach { terminal_id, since_seq, cols, rows }
terminal.input  { terminal_id, data_b64 }
terminal.resize { terminal_id, cols, rows }
terminal.detach { terminal_id }
terminal.close  { terminal_id, kill }

server -> client
terminal.opened { terminal_id, status }
terminal.output { terminal_id, seq, data_b64 }
terminal.exit   { terminal_id, code, signal }
terminal.error  { request_id?, terminal_id?, code, message, action, detail? }
```

Error codes should be stable and shared across daemon, gateway, and Web UI.
Minimum codes:

- `not_found`
- `not_active_writer`
- `exited`
- `backend`
- `invalid_base64`
- `invalid_json`
- `unsupported_platform`
- `terminal_disabled`
- `missing_terminal_token`
- `daemon_unavailable`
- `terminal_socket_unavailable`
- `remote_gateway_unreachable`
- `remote_auth_failed`

## Execution Policy

Main sequence:

1. Complete 1.1 UX contracts and Web behavior.
2. Align 1.2 process architecture and naming.
3. Run cross-layer verification and update docs.

Implementation rules:

- Keep each batch small and verifiable: one behavior change, one focused test or
  inspection, one architecture-boundary check.
- Do not introduce compatibility compromises for old runtime names when working
  on the 1.2 architecture pass.
- Do not mutate provider history files.
- Do not let gateway hold PTY handles, child handles, terminal registries,
  active-writer state, or scrollback.
- Do not make full TUI terminal UI part of v1 acceptance.
- Keep remote terminal credentials server-side.
- Prefer contract updates before spreading implementation across daemon,
  gateway, and Web UI.

Suggested implementation waves:

1. UX contracts:
   - Stabilize readiness DTOs, structured errors, terminal summary fields, and
     Web client types.
   - Verify every unavailable state has an action and no inert controls.
2. Web behavior:
   - Implement refresh/no-auto-attach behavior, explicit attach, replay with
     `since_seq: null`, clearer Detach/Kill copy, and Kill confirmation.
   - Verify browser refresh and Web restart do not kill daemon-owned terminals.
3. Architecture alignment:
   - Apply daemon/gateway process vocabulary from the runtime architecture
     plan.
   - Remove `core`/`web` runtime concepts from CLI, settings, protocol, logs,
     health output, and docs.
4. Hardening:
   - Add regression tests, architecture scans, docs, and residual-risk notes.

## Main Agent Role

The main agent is the orchestrator and integration owner.

Responsibilities:

- Preserve the user's goal and acceptance criteria.
- Keep this document and `.ai/runtime-process-architecture-plan.md` aligned.
- Keep main context focused on decisions, contracts, risks, and verification.
- Assign narrow, non-overlapping tasks to subagents.
- Review subagent output before accepting it into the main plan or codebase.
- Detect and correct drift from daemon-owned lifecycle and gateway-only edge
  responsibilities.
- Merge implementation work in dependency order.

The main agent must not outsource final judgment. Subagents can research,
design, implement, or verify bounded areas, but the main agent remains
responsible for architecture consistency and acceptance.

## Subagent Roles

UX Agent:

- Owns Web terminal user journey, state model, readiness messaging, structured
  errors, refresh/reattach behavior, and Detach/Kill experience.
- Must not redefine backend protocol independently.

Architecture Agent:

- Owns daemon/gateway/tui/gui process boundaries, runtime naming, settings
  naming, protocol naming, and crate/process responsibility checks.
- Must remove compatibility-oriented `core`/`web` runtime assumptions from the
  target plan.

Daemon Runtime Agent:

- Owns `TerminalManager`, `TerminalSession`, state machine, active writer,
  attach/detach, replay, kill, and PTY adapter boundaries.
- Must prove behavior with fake backend tests before relying only on real PTY
  behavior.

Gateway Agent:

- Owns browser HTTP/WebSocket edge behavior, auth checks, structured error
  mapping, and bridge-to-daemon behavior.
- Must not retain terminal runtime state.

Web UI Agent:

- Owns React/xterm UI, terminal list presentation, readiness controls,
  structured error display, refresh behavior, and terminal token UX.
- Must keep write-capable controls out of read-only/share contexts.

Remote Agent:

- Owns local-daemon-to-remote-gateway-to-remote-daemon terminal proxy behavior.
- Must ensure remote terminal tokens remain server-side and browse-only remotes
  surface as terminal-unavailable before launch.

Verification and Docs Agent:

- Owns tests, command results, architecture scans, docs updates, and residual
  risk notes.
- Must report any drift from this plan instead of silently normalizing it.

Every subagent handoff should include:

- Conclusion.
- Files touched or inspected.
- Risks.
- Verification performed.
- Any architecture drift found.

## Drift Checks

- Gateway must not construct or own long-lived `AppService` business state for
  runtime authority.
- Gateway must not own PTY/session lifecycle.
- Daemon must be the only owner of terminal lifecycle.
- `coca-app` must not own PTY handles or provider child processes.
- `coca-core` must remain provider/session/launch primitives.
- Browser JavaScript must not receive remote terminal tokens.
- Unsupported sessions must show unavailable state before launch.
- Resume/Fork/Attach/Detach/Kill must remain the core user journey.
- Web refresh must preserve daemon-owned processes and require explicit attach.

## Test Plan

Rust tests:

- Settings defaulting, validation, terminal token redaction, and remote
  `terminal_token` redaction.
- Terminal readiness DTOs for all unavailable states.
- Structured terminal errors from daemon and gateway.
- Terminal open rejects disabled terminal, missing token, missing session,
  browse-only remote, and unsupported platform.
- `TerminalManager` keeps sessions alive across detach/reattach while daemon
  remains running.
- Only one active writer can send input/resize in v1.
- Slow client queues do not block PTY output readers.
- Remote proxy chooses the correct remote and keeps remote credentials
  server-side.
- Gateway forwards terminal streams and does not retain lifecycle state.

Frontend tests:

- Session list/detail render readiness and action states correctly.
- Browser refresh shows daemon terminal list and does not auto-attach.
- Explicit attach replays output with `since_seq: null`.
- Detach leaves the terminal running.
- Kill asks for confirmation and then terminates the terminal session.
- Read-only/share contexts do not show terminal write controls.
- Terminal auth errors are distinct from read/share auth errors.

Verification commands:

```sh
cargo xtask verify
cd app/web && npm run test
cd app/web && npm run build
```

## Acceptance Criteria

- A Web user can Resume/Fork a local or terminal-capable remote session and use
  a real terminal.
- A Web user can refresh, leave, and later explicitly reattach to a daemon-owned
  terminal without killing the provider process.
- A Web user can clearly distinguish unavailable terminal states and the next
  action to fix them.
- Detach and Kill have unambiguous, correct behavior.
- Gateway remains an edge process and daemon remains the runtime authority.
- TUI/GUI future support is enabled by daemon APIs without making full TUI
  terminal UI part of v1.

## Deferred

- Daemon crash/restart recovery for already-running PTYs.
- Multiple active writers.
- tmux-style panes/windows/layouts.
- Full alternate-screen restoration through a headless terminal model.
- Per-open launch option editing.
- Advanced browser shortcut mapping.
- Terminal output search/indexing.

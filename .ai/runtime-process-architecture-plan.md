# Runtime Process Architecture Plan

## Goal

Define the target runtime process architecture for `coca`.

Architecture discussions use the process view by default:

```text
Processes:    coca daemon / coca gateway / coca tui / coca gui
Code layers:  coca-app / coca-core
View code:    app/web
```

The central decision is:

```text
core and app are code layers, not runtime process roles.
```

This plan does not preserve compatibility with old runtime names. The target
architecture should be chosen for correctness, clarity, and extensibility.

## Target Process View

Local runtime:

```text
                         Local Runtime

+-------------+        HTTP         +----------------+
| Browser     | <-----------------> | coca gateway   |
| app/web     |                     | browser edge   |
+-------------+                     +-------+--------+
                                            |
                                            | local IPC / JSON-RPC / stream
                                            v
+-------------+      local IPC       +----------------+
| coca tui    | <------------------> | coca daemon    |
+-------------+                      | local service  |
                                     | authority      |
+-------------+      local IPC       |                |
| coca gui    | <------------------> |                |
+-------------+                      +-------+--------+
                                             |
                                             v
                                      coca-app crate
                                      use cases / DTOs
                                             |
                                             v
                                      coca-core crate
                                      providers / models
                                             |
                                             v
                              provider histories / settings
```

Canonical request paths:

```text
Browser -> coca gateway -> coca daemon -> coca-app -> coca-core
coca tui -> coca daemon -> coca-app -> coca-core
coca gui -> coca daemon -> coca-app -> coca-core
```

Terminal path:

```text
Browser/TUI/GUI
  -> gateway when browser HTTP/WebSocket is required
  -> daemon terminal stream
  -> daemon-owned TerminalManager
  -> PTY/PTS + provider child process
```

Remote terminal-capable machine:

```text
local frontend
  -> local daemon
  -> remote coca gateway
  -> remote coca daemon
  -> remote PTY/PTS + provider CLI
```

## Runtime Roles

### `coca daemon`

`daemon` is the local authoritative service process.

Responsibilities:

- Own session catalog access through `coca-app`/`coca-core`.
- Own settings reads and mutations exposed to frontends.
- Own share link generation and runtime actions.
- Own launch/Resume/Fork orchestration.
- Own terminal lifecycle, PTY/PTS handles, provider child processes,
  attach/detach state, active writer, output sequence, scrollback, and kill.
- Expose local IPC/RPC/stream APIs for gateway, TUI, GUI, and CLI helpers.

Non-goals:

- Do not serve browser assets.
- Do not expose public HTTP by default.
- Do not contain frontend rendering code.

### `coca gateway`

`gateway` is the browser-facing HTTP/WebSocket edge process.

Responsibilities:

- Serve built `app/web` assets.
- Expose browser-friendly HTTP APIs under `/api/*`.
- Handle browser auth/token checks.
- Bridge HTTP requests to daemon RPC.
- Bridge terminal WebSocket streams to daemon terminal streams.
- Own edge concerns such as HTTP status mapping, CORS, static assets, health
  output, and share URL serving.

Non-goals:

- Do not parse provider histories.
- Do not construct or own authoritative `AppService` runtime state.
- Do not mutate settings directly.
- Do not own PTY handles, provider child processes, terminal registries,
  active-writer state, or scrollback.
- Do not manage daemon lifecycle.

If daemon is unavailable, gateway returns `503` with a structured error. It
does not fall back to in-process app/core calls.

### `coca tui`

`tui` is a terminal frontend process.

Responsibilities:

- Present session lists, details, settings, terminal summaries, and actions.
- Consume daemon APIs for data and runtime actions.
- Future terminal UI attaches to daemon-owned `TerminalSession` objects.

Non-goals:

- Do not parse provider histories directly in the target architecture.
- Do not be the only owner of provider process execution.
- Do not own terminal lifecycle.

### `coca gui`

`gui` is a future desktop frontend process. It follows the same authority
boundary as TUI:

```text
coca gui -> coca daemon -> coca-app -> coca-core
```

### `coca-app`

`coca-app` is the application use-case crate.

Responsibilities:

- User-visible workflows and DTOs.
- Session list/detail use cases.
- Config summaries and settings orchestration.
- Share link use cases.
- Launch/Resume/Fork resolution.
- Terminal readiness DTOs, request/response DTOs, and launch resolution.

Non-goals:

- Do not own process lifecycle.
- Do not own PTY handles or child process handles.
- Do not contain frontend rendering.

### `coca-core`

`coca-core` is the domain/data primitive crate.

Responsibilities:

- Normalized session models.
- Provider loaders and parsers.
- Session catalog primitives.
- Settings persistence primitives.
- Launch command construction primitives.

Non-goals:

- Do not expose a runtime process.
- Do not serve HTTP.
- Do not own app workflows or frontend DTOs.
- Do not mutate provider history files.

## Target Public Interfaces

CLI commands:

```text
coca daemon
coca gateway
coca tui
coca gui
```

Runtime commands named `coca core` or `coca web` are not part of the target
architecture.

Settings naming:

```json
{
  "daemon": {
    "socket": "~/.config/coca/daemon.sock",
    "terminal_socket": "~/.config/coca/daemon.terminal.sock"
  },
  "gateway": {
    "bind": "127.0.0.1:8787"
  },
  "terminal": {
    "enabled": false,
    "token": "<generated>"
  }
}
```

Protocol naming:

- Use `daemon.ping`, not `core.ping`.
- Use neutral or daemon-oriented type names such as `DaemonPingResult` or
  `ServiceHealth`.
- Keep business methods named by capability, for example `sessions.list`,
  `settings.get`, `settings.update`, `share.url`, `launch.prepare`,
  `terminal.list`, and `terminal.get`.

Gateway HTTP:

- `/api/v1/health` reports `service: "coca-gateway"` and daemon readiness.
- Browser APIs are implemented by daemon RPC proxying.
- Terminal WebSocket endpoints only authenticate and bridge to daemon streams.
- Daemon-unavailable responses use structured errors:

```json
{
  "code": "daemon_unavailable",
  "message": "coca daemon is not available.",
  "action": "Start coca daemon and retry.",
  "detail": "optional diagnostic text"
}
```

Crate naming target:

- `crates/coca-daemon`: daemon process host and local service adapters.
- `crates/coca-gateway`: browser HTTP/WebSocket gateway host.
- `crates/coca-app`: use cases, DTOs, and frontend-facing workflows.
- `crates/coca-core`: provider/session/settings/launch primitives.
- `app/web`: browser React application.

## Architecture Changes To Make

CLI and process entry:

- Replace runtime commands with `daemon`, `gateway`, `tui`, and future `gui`.
- Route default interactive behavior through the TUI frontend command path.
- Remove `core` and `web` as runtime command concepts.
- Rename default daemon socket paths away from `core.sock`.

Daemon:

- Stop re-exporting `coca_core::server` as a process entrypoint.
- Host all authoritative local APIs: sessions, settings, share, launch, and
  terminal.
- Keep terminal stream serving inside daemon and backed by `TerminalManager`.
- Keep platform-specific IPC and PTY behavior behind small adapters.

Gateway:

- Rename Web service host semantics to gateway.
- Move all direct business API handling behind daemon RPC calls.
- Keep only browser edge responsibilities in the gateway crate.
- Split routing, auth/error mapping, daemon RPC proxying, and terminal
  WebSocket bridging into separate modules.

Protocol and settings:

- Rename process-oriented `Core*` protocol names to daemon/service names.
- Rename settings groups to `daemon.*` and `gateway.*`.
- Keep terminal settings under `terminal.*`.
- Keep provider/session domain types in `coca-core`; keep frontend DTOs in
  `coca-app`.

Frontend paths:

- Web browser app talks only to gateway HTTP/WebSocket APIs.
- TUI and future GUI talk to daemon through local client boundaries.
- No frontend process reads provider histories or owns terminal lifecycle.

Docs and durable memory:

- Use daemon/gateway/tui/gui for process names.
- Use app/core only for code-layer names.
- Remove architecture language that treats `core` or `web` as runtime services.

## Execution Policy

Main sequence:

1. Lock public names and contracts:
   - CLI commands.
   - Settings groups.
   - Protocol method/type names.
   - Health and error schemas.
2. Move authority to daemon:
   - Ensure sessions/settings/share/launch/terminal APIs originate from daemon.
   - Ensure gateway has no direct app/core authority fallback.
3. Rename gateway process surface:
   - CLI command, logs, health output, package/crate naming, docs.
4. Route frontends through correct boundaries:
   - Web through gateway.
   - TUI/GUI through daemon.
5. Split oversized modules:
   - Gateway routing/auth/error/proxy/terminal bridge.
   - Daemon RPC/state/terminal backend/terminal stream.
6. Verify architecture:
   - Run tests.
   - Scan for old runtime vocabulary.
   - Inspect boundary violations.

Implementation rules:

- Do not preserve old runtime command names for compatibility.
- Do not add fallback paths that bypass daemon authority.
- Do not let gateway manage daemon lifecycle.
- Do not move provider parsing out of `coca-core`.
- Do not move user-facing use cases out of `coca-app`.
- Do not let frontend processes own PTY/process lifecycle.
- Keep changes surgical within each batch.

## Main Agent Role

The main agent is the architecture owner and integration controller.

Responsibilities:

- Keep the process-view architecture clear and consistent.
- Preserve the distinction between process roles and code layers.
- Keep `.ai/terminal-integration-v1.md` and this file aligned.
- Assign narrow work to subagents and reject drift.
- Review subagent output for boundary violations.
- Decide merge order and final acceptance.
- Keep the main context focused on goals, contracts, risks, and verification.

The main agent must not delegate final architectural judgment. Subagents can
produce designs, implementation slices, or verification reports, but the main
agent decides whether they satisfy the target model.

## Subagent Roles

Architecture Subagent:

- Owns process vocabulary, CLI/runtime naming, settings naming, protocol
  naming, and target diagrams.
- Must remove compatibility-oriented runtime assumptions.

Daemon Subagent:

- Owns daemon API surface, daemon state, local IPC, terminal stream ownership,
  and service health.
- Must ensure daemon remains the only runtime authority.

Gateway Subagent:

- Owns HTTP routing, WebSocket bridging, auth, structured errors, static asset
  serving, and daemon RPC proxying.
- Must ensure gateway has no direct provider/settings/share/launch authority.

Frontend Subagent:

- Owns Web/TUI/GUI client-boundary changes and user-visible process labels.
- Must ensure frontends consume gateway or daemon APIs rather than provider
  internals.

Protocol/Settings Subagent:

- Owns method/type naming, settings schemas, TypeScript/Rust DTO alignment, and
  config summaries.
- Must keep process names out of code-layer concepts.

Verification Subagent:

- Owns tests, command output, vocabulary scans, architecture-boundary scans,
  docs consistency, and residual-risk notes.
- Must report drift instead of silently adapting to it.

Each subagent handoff should include:

- Conclusion.
- Files inspected or touched.
- Risks.
- Verification performed.
- Drift found or explicitly not found.

## Drift Checks

- No target docs or user-visible runtime output should call `core` a process.
- No target docs or user-visible runtime output should call `web` the service
  process; use `gateway`.
- Gateway must not construct authoritative `AppService` state for ordinary
  business APIs.
- Gateway must not own terminal runtime state.
- Daemon must own local runtime authority.
- TUI/GUI must consume daemon APIs.
- `coca-app` must stay use-case/DTO/workflow oriented.
- `coca-core` must stay provider/session/settings/launch primitive oriented.
- Provider histories must remain read-only.

## Test Plan

CLI and naming:

- Help output lists target process commands.
- Startup output, logs, and health responses use daemon/gateway vocabulary.
- Default socket paths use daemon naming.

Daemon:

- `daemon.ping` returns daemon/service health.
- Sessions, settings, share, launch, and terminal methods are served by daemon.
- Terminal lifecycle remains daemon-owned across detach/reattach.

Gateway:

- Gateway session/config/share/terminal APIs proxy through daemon.
- Daemon unavailable returns `503` structured errors.
- Gateway tests prove no terminal registry, PTY handle, child process handle,
  active writer, or scrollback is stored in gateway state.

Protocol and settings:

- Rust and TypeScript DTOs agree on settings, health, readiness, and terminal
  error shapes.
- Old runtime-oriented names are absent from target protocol and settings
  schemas.

Architecture scans:

```sh
rg -n "coca core|coca web|core\\.ping|core\\.sock|core\\.bind|frontend/core|CoreClient" src crates app docs .ai
rg -n "AppService" crates/coca-gateway crates/coca-web
```

Verification commands:

```sh
cargo xtask verify
cd app/web && npm run test
cd app/web && npm run build
```

## Acceptance Criteria

- The runtime process model is understandable from names alone:
  daemon is authority, gateway is browser edge, TUI/GUI are frontends.
- `coca-app` and `coca-core` are never required to be understood as processes.
- Web requests cannot bypass daemon authority for business/runtime behavior.
- Terminal lifecycle cannot be owned by gateway or frontend processes.
- Future TUI/GUI frontends can be added by consuming daemon APIs without
  duplicating provider parsing or terminal lifecycle logic.

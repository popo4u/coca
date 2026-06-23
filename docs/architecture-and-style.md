# Architecture and Style

This file owns project-specific architecture and coding constraints for `coca`.

## Project Shape

`coca` is a Rust workspace. The root `coca` package is the CLI shell, while reusable capabilities live in focused crates under `crates/`.

Prefer small modules over large files. Keep responsibilities separated so new coder-agent providers and new frontends can be added without rewriting the TUI.

## Architecture Rules

- Keep provider parsing read-only. Never mutate `~/.codex`, `~/.claude`, or provider history files.
- Keep provider logic separate from frontend and app workflow logic. Providers load normalized session data only.
- Keep user-visible workflows in `coca-app`, not in `coca-core`. Session sharing, config summaries, frontend DTOs, launch orchestration, and future terminal lifecycle are app-layer behavior.
- Keep launch command construction primitives in core, but defaults, permissions, and frontend-facing orchestration belong in app.
- Keep process execution isolated behind a small module. Unix-only APIs must be guarded with `#[cfg(unix)]`; Windows must keep a working fallback.
- Keep protocol and IPC crates free of UI code and provider parsing.
- Runtime authority belongs to `coca daemon`. TUI/Web/GUI frontends should consume daemon APIs through a client/API boundary instead of calling provider, settings, share, launch, or terminal internals directly.
- The browser-facing Web service is a gateway. It serves JSON APIs, reserved stream endpoints, WebSocket bridges, and static assets built from `app/web`; it must not own provider parsing, settings authority, launch authority, or terminal lifecycle.
- Use `Path`/`PathBuf` for paths. Do not hard-code platform-specific separators.
- Keep cross-platform behavior in mind for Linux, macOS, and Windows.

## Crate Ownership

- `crates/coca-core/`: normalized models, provider loaders, session catalog primitives, settings persistence primitives, remote loading, and launch construction primitives.
- `crates/coca-app/`: user-visible use cases and frontend/API DTOs: session browse/detail, config summaries, share links, launch orchestration, and future terminal lifecycle.
- `crates/coca-protocol/`: JSON-RPC wire types and method names for daemon/frontend communication.
- `crates/coca-ipc/`: local IPC framing and transport helpers.
- `crates/coca-daemon/`: local authoritative service host, RPC/server adapters, and daemon-owned runtime state such as terminal lifecycle.
- `crates/coca-tui/`: app state, key handling, rendering, and view helpers for the terminal frontend. It owns the frontend daemon-client contract.
- `crates/coca-web/`: browser gateway host for HTTP APIs, WebSocket bridges, and static assets for the React Web frontend.
- `app/web/`: React + TypeScript browser frontend. It talks only to gateway APIs.
- `app/gui/`: reserved for a future desktop GUI frontend.
- `app/tui/`: reserved for a possible future home for the terminal frontend; today TUI code remains in `crates/coca-tui/`.
- `src/`: root CLI shell, frontend RPC client adapter, and final process execution bridge.
- `xtask/`: project automation.

## Coding Style

- Preserve behavior unless the user explicitly requests a behavior change.
- Add or move tests with the module that owns the behavior.
- Prefer explicit domain types over raw tuples or ad hoc strings.
- Keep UI state transitions testable without needing a terminal.
- Avoid broad refactors unrelated to the current task.
- Use existing local helpers and patterns before adding new abstractions.
- Add abstractions only when they remove real duplication or clarify a repeated concept.

## Provider Expectations

- A provider should return normalized `Session` values.
- Transcript extraction should preserve full user/assistant text transcript where practical.
- Provider-specific command details belong in launch construction, not in provider parsers.
- Provider storage is always a read-only input.

When adding a provider:

1. Add provider-specific parsing under `crates/coca-core/src/providers/`.
2. Normalize data into `coca_core::model::Session`.
3. Add provider command construction in `crates/coca-core/src/launch.rs`.
4. Keep provider storage read-only.
5. Add focused parser and launch tests.

## TUI Expectations

- Keep key handling, app state, and rendering separated.
- Rendering helpers should be pure where practical.
- Do not let modal-specific keys leak into the main list behavior.
- Use the daemon client/RPC/app boundary for business behavior instead of direct provider, settings, share, launch, or terminal internals.
- Preserve existing keybindings unless the user requests changes.
- Keep state transitions covered by tests where practical.

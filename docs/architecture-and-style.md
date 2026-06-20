# Architecture and Style

This file owns project-specific architecture and coding constraints for `coca`.

## Project Shape

`coca` is a Rust workspace. The root `coca` package is the CLI shell, while reusable capabilities live in focused crates under `crates/`.

Prefer small modules over large files. Keep responsibilities separated so new coder-agent providers and new frontends can be added without rewriting the TUI.

## Architecture Rules

- Keep provider parsing read-only. Never mutate `~/.codex`, `~/.claude`, or provider history files.
- Keep provider logic separate from frontend logic. Providers load normalized session data; TUI/GUI frontends render and edit UI state only.
- Keep launch command construction in core, separate from rendering and key handling.
- Keep process execution isolated behind a small module. Unix-only APIs must be guarded with `#[cfg(unix)]`; Windows must keep a working fallback.
- Keep protocol and IPC crates free of UI code and provider parsing.
- TUI/GUI frontends should consume core through the RPC/client boundary. In-process clients may use the same JSON-RPC router as a transport optimization, but frontend code should not call provider, settings, share, or launch internals directly.
- Use `Path`/`PathBuf` for paths. Do not hard-code platform-specific separators.
- Keep cross-platform behavior in mind for Linux, macOS, and Windows.

## Crate Ownership

- `crates/coca-core/`: normalized models, provider loaders, session catalog, settings, share, remote loading, launch planning, and frontend-facing core use cases.
- `crates/coca-protocol/`: JSON-RPC wire types and method names for core/frontend communication.
- `crates/coca-ipc/`: local IPC framing and transport helpers.
- `crates/coca-daemon/`: core process host and RPC/server adapters.
- `crates/coca-tui/`: app state, key handling, rendering, and view helpers for the terminal frontend. It owns the `CoreClient` frontend contract.
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
- Use the `CoreClient`/RPC boundary for business behavior instead of direct provider, share, settings, or launch internals.
- Preserve existing keybindings unless the user requests changes.
- Keep state transitions covered by tests where practical.

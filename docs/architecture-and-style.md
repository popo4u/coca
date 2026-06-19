# Architecture and Style

This file owns project-specific architecture and coding constraints for `coca`.

## Project Shape

`coca` is a single Rust application crate. Keep it that way unless the user explicitly asks for a workspace split.

Prefer small modules over large files. Keep responsibilities separated so new coder-agent providers can be added without rewriting the TUI.

## Architecture Rules

- Keep provider parsing read-only. Never mutate `~/.codex`, `~/.claude`, or provider history files.
- Keep provider logic separate from TUI logic. Providers load normalized session data; the TUI renders and edits UI state only.
- Keep launch command construction separate from rendering and key handling.
- Keep process execution isolated behind a small module. Unix-only APIs must be guarded with `#[cfg(unix)]`; Windows must keep a working fallback.
- Use `Path`/`PathBuf` for paths. Do not hard-code platform-specific separators.
- Keep cross-platform behavior in mind for Linux, macOS, and Windows.

## Module Ownership

- `src/model.rs`: normalized provider/session data.
- `src/providers/`: read-only loaders for provider history.
- `src/launch.rs`: resume, execute, and fork command construction.
- `src/process.rs`: Unix `exec` and non-Unix child-process fallback.
- `src/cli.rs`: command-line parsing and default provider roots.
- `src/tui/`: app state, key handling, rendering, and view helpers.
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

1. Add provider-specific parsing under `src/providers/`.
2. Normalize data into `model::Session`.
3. Add provider command construction in `src/launch.rs`.
4. Keep provider storage read-only.
5. Add focused parser and launch tests.

## TUI Expectations

- Keep key handling, app state, and rendering separated.
- Rendering helpers should be pure where practical.
- Do not let modal-specific keys leak into the main list behavior.
- Preserve existing keybindings unless the user requests changes.
- Keep state transitions covered by tests where practical.

# AGENTS.md

Guidance for agents working in this repository.

## Project Shape

`coca` is a Rust terminal UI for browsing and launching local coder-agent sessions. It supports Codex and Claude today and should stay easy to extend to new providers.

Keep the crate as a single application crate unless the user explicitly asks for a workspace split. Prefer small modules over large files.

## Architecture Rules

- Keep provider parsing read-only. Never mutate `~/.codex`, `~/.claude`, or provider history files.
- Keep provider logic separate from TUI logic. Providers load normalized session data; the TUI renders and edits UI state only.
- Keep launch command construction separate from rendering and key handling.
- Keep process execution isolated behind a small module. Unix-only APIs must be guarded with `#[cfg(unix)]`; Windows must keep a working fallback.
- Use `Path`/`PathBuf` for paths. Do not hard-code platform-specific separators.
- Keep cross-platform behavior in mind for Linux, macOS, and Windows.

## Verification

The canonical full verification command is:

```sh
cargo xtask verify
```

While implementing, run focused checks locally when they are useful:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

When subagents are available, delegate full `cargo xtask verify` to a verifier subagent after implementation. The verifier should report pass/fail and a concise failure summary only, so the main session context stays focused.

If subagents are unavailable, run `cargo xtask verify` locally.

## Coding Style

- Preserve behavior unless the user explicitly requests a behavior change.
- Add or move tests with the module that owns the behavior.
- Prefer explicit domain types over raw tuples or ad hoc strings.
- Keep UI state transitions testable without needing a terminal.
- Avoid broad refactors unrelated to the current task.

## Provider Expectations

- A provider should return normalized `Session` values.
- Transcript extraction should preserve full user/assistant text transcript where practical.
- Provider-specific command details belong in launch construction, not in provider parsers.

## TUI Expectations

- Keep key handling, app state, and rendering separated.
- Rendering helpers should be pure where practical.
- Do not let modal-specific keys leak into the main list behavior.
- Preserve existing keybindings unless the user requests changes.

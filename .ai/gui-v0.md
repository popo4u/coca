# Session Read-Only Web Sharing Goal and v0 Plan

## Final Goal

Enable read-only sharing for local coder-agent sessions. The target workflow is that two teammates can each expose selected sessions from their own PC, then share a URL that opens the session in a browser for work discussion.

The shared view must be read-only. It must not mutate provider histories, start resume/fork actions, or expose write-capable controls. This feature should fit `coca`'s existing role as a local and remote session browser for Codex and Claude, while staying easy to extend to future providers.

## Current State

- `coca` is a single Rust application crate.
- Provider loaders already normalize Codex and Claude history into `Session` values with metadata and full transcript text where practical.
- Provider history is treated as read-only input.
- The TUI can browse local and configured remote sessions, inspect details, and open transcript views.
- Existing remote support is a read-only TCP JSON-RPC service for `sessions.list`.
- v0 now includes a browser-facing read-only HTTP share service, a single-session share URL format, a static Web session detail page, and a TUI `u` shortcut to show share URLs for local sessions.

## v0 Product Decisions

- Network scope: local network direct access.
- Sharing granularity: single session link.
- Access control: URL token.
- Service shape: independent HTTP server command.
- TUI entry point: selected local session can show a share URL.
- Web content: metadata plus full transcript.
- Exclusions for v0: no public list page, no login system, no HTTPS management, no public relay, no audit log, no write operations.

## Proposed v0 Interface

HTTP server command:

```sh
coca share serve --bind 0.0.0.0:8787 --token <secret> --provider all
```

The command should also support `--codex-home` and `--claude-home`, with the same semantics as the existing CLI options.

Single-session URL:

```text
http://<host>:8787/s/<provider>/<encoded-session-id>?token=<secret>
```

Example:

```text
http://192.168.1.20:8787/s/codex/<encoded-session-id>?token=<secret>
```

Settings for TUI URL generation:

```json
{
  "share": {
    "base_url": "http://192.168.1.20:8787",
    "token": "<secret>"
  }
}
```

TUI behavior:

- Add a `u` key for URL sharing.
- For a selected local session, show a modal containing the generated share URL.
- If `share.base_url` or `share.token` is missing, show a clear status message.
- If a remote session is selected, show that remote sessions cannot be shared by the current machine in v0.

## Implementation Plan

- Add a `share` module that owns HTTP request parsing, routing, token checks, URL building, HTML rendering, and server startup.
- Reuse `providers::load_sessions` to resolve sessions on demand from local provider histories.
- Keep `src/model.rs` as the normalized data contract; do not add provider-specific Web rendering paths.
- Add `share` settings to `Settings` with default empty values so existing settings files remain valid.
- Add `Command::Share(ShareArgs)` and `ShareCommand::Serve(ShareServeArgs)` in the CLI.
- Wire `main.rs` so `coca share serve ...` starts the HTTP service.
- Keep the existing TCP JSON-RPC `client serve` behavior unchanged.
- Add a TUI share URL modal/state path without changing resume/fork behavior.
- Update README and `docs/dev.md` with setup, examples, and security notes.

## Web Page Requirements

- Show title, provider, session id, cwd, model, created time, updated time, and transcript.
- Do not show `source_path`, `resume_program`, or `resume_args`.
- Preserve message newlines and whitespace.
- Escape all dynamic HTML content.
- Return `Cache-Control: no-store`.
- Return `401` for missing or invalid token.
- Return `404` for unknown provider or unknown session.
- Return `405` for unsupported methods.
- Use static HTML/CSS with no JavaScript for v0.

## Test Plan

- Settings parse tests for missing and populated `share` config.
- URL builder tests for percent-encoding session id and token.
- HTTP route tests for valid session rendering.
- HTTP auth tests for missing and invalid token.
- HTTP not-found tests for unknown provider and unknown session id.
- HTML escaping tests for session metadata and transcript text.
- TUI key handling tests for local session, remote session, and missing share settings.
- Focused verification: `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`.
- Full verification: `cargo xtask verify`.

## Important Assumptions

- The user is responsible for choosing a strong token and controlling LAN exposure.
- v0 does not try to infer a LAN IP address; `share.base_url` is explicit.
- The HTTP server exposes only local sessions from the machine where it runs.
- Sharing remote sessions through a local TUI is deferred until there is an explicit forwarding or remote-share design.

## Implementation Status

- Implemented on branch `gui` after rebasing onto `main` at `476f5db`.
- Added `coca share serve --bind <host:port> --token <secret>`.
- Added `share.base_url` and `share.token` settings.
- Added TUI settings editor entries for `share.base_url` and `share.token`.
- Added TUI `u` share URL modal for local sessions.
- Added route, token, HTML escaping, settings, and TUI tests.
- Verified with `cargo xtask verify`.

## Web Session Rendering v1 Baseline

- Current implementation is only the first basic Web rendering baseline, not the final Codex/Claude-like transcript experience.
- Added a read-only session page layout with a main transcript column and a sticky desktop metadata rail.
- Kept the title panel the same width as the transcript and clamped long titles to two lines with the full title in the HTML `title` attribute.
- Rendered the first prompt as its own panel and omitted the duplicate first user transcript item when it exactly matches the prompt.
- Added a small semantic transcript rendering layer in `src/share.rs` so later Web, GUI, and mobile views can share message classification behavior.
- Classified provider context wrappers and subagent notifications as structured blocks while escaping all dynamic HTML content.
- Added focused tests for first-prompt de-duplication, context rendering, subagent notification rendering, and title attribute escaping.
- Next rendering work should focus on making transcript messages substantially closer to the original Codex/Claude app experience: richer role treatment, better tool/action blocks, improved markdown/code presentation, stronger spacing hierarchy, and mobile review.
- Verified the implementation with `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo xtask verify`.

# Superseded / Deprecated Notice

This is a historical plan. Do not use the runtime vocabulary below as the
current target architecture.

Current authoritative plans:

- `.ai/runtime-process-architecture-plan.md`
- `.ai/terminal-integration-v1.md`

Current process view uses `coca daemon`, `coca gateway`, `coca tui`, and
`coca gui`. `coca-app` and `coca-core` are code layers only, not runtime
process roles. Historical terms below such as `coca core`, `coca web`,
`CoreClient`, `core.bind`, and `frontend/core` are preserved for traceability.

# Web/App Refactor Status

## Goal

Move the browser frontend into `app/web`, keep future frontend slots clear under
`app/`, and route user-visible Web behavior through `coca-app` instead of
putting UI DTOs or HTML rendering in `coca-core`.

## Current Architecture

1. `app/web/` owns the React + TypeScript browser UI.
2. `crates/coca-web/` owns the HTTP JSON API and static asset hosting.
3. `crates/coca-app/` owns app-layer DTOs and user-visible workflows such as
   session browse/detail, config summary, share URL generation, launch defaults,
   and AI settings updates.
4. `crates/coca-core/` remains focused on normalized models, provider loading,
   settings persistence primitives, remote loading, launch construction, and the
   derived SQLite store.
5. `app/gui/` and `app/tui/` are reserved future locations. The terminal UI
   remains in `crates/coca-tui` for now.

## Implemented Behavior

- `coca web` serves built assets from `app/web/dist`.
- Web APIs live under `/api/v1/*`; normal APIs require daemon-backed account
  bearer tokens and route scopes.
- Public share reads use per-link `/api/v1/share/<link-id>?share_token=...`
  tokens with independent expiry and revocation.
- React owns sessions, config, account/profile access, and session detail routes.
- Session detail is split into title, metadata, and transcript sections.
- Transcript rendering starts from `first_user_message` and uses a timeline UI
  with distinct user, assistant, context, and event treatments.
- Markdown transcript entries default to preview mode, can switch to raw mode,
  and each timeline entry can collapse or expand.
- Config includes theme switching and OpenAI-compatible AI summary settings.
- AI API keys are redacted in summaries; blank key updates keep the existing
  stored key, and explicit clear removes it.
- `coca-core` no longer exposes the legacy Rust-rendered `/s/...` share page.
  Share links now point to the React Web route.
- A derived SQLite store records refreshed session payloads and future summary
  cache rows without mutating provider histories.

## Verification Used

- `npm run test`
- `npm run build`
- `cargo fmt --all --check`
- `cargo xtask verify`
- Playwright visual/DOM check against a real local session at desktop and mobile
  widths, confirming no horizontal overflow in session detail or config.

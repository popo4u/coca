# Account/Auth MVP Implementation Tracker

## Goal

Implement the Account/auth surfaces described in `.ai/gap.md` as the primary
local account system: email/password first-account signup and login, profile
editing, device sessions, scoped access token creation, share-link management,
and token revocation.

## Architecture

- Browser talks to gateway APIs only.
- Gateway validates daemon-backed account tokens for normal APIs; legacy
  gateway/share tokens do not authorize ordinary API calls.
- Daemon is the authoritative service host.
- `coca-app` owns account/auth workflows and API DTO behavior.
- `coca-core` owns SQLite persistence primitives and auth/share tables.
- Terminal browser actions are authorized by account scopes, not a separate
  browser-entered terminal token.
- Public sharing uses per-link read tokens with independent expiry/revocation.

## Web Design Reference

Use root `index.html` as the visual and interaction reference for auth/account UI, especially:

- sign in / signup panels
- profile header
- Security / Access token rows
- Device / Browser Sessions rows
- disabled SSO states

## Worker Split

- Rust/API worker: protocol, core storage, app workflows, daemon dispatch, gateway routes/tests.
- Web worker: API client/types, auth gate, profile/security UI, route/nav integration, CSS.

## Acceptance

- `cargo test -p coca-core -p coca-app -p coca-protocol -p coca-daemon -p coca-web --lib`
- `cd app/web && npm run test`
- `cd app/web && npm run build`
- `cargo xtask verify`
- `git diff --check`

## Current Status

- Auth storage was rebuilt to schema v3 for users, device sessions, scoped PATs,
  and per-link share tokens. Historical auth/share credential compatibility is
  intentionally not preserved.
- First-account signup no longer uses `settings.share.token` as a bootstrap
  credential.
- Normal gateway APIs require account bearer tokens plus route scopes:
  `sessions.read`, `share.manage`, `account.manage`, `tokens.manage`,
  `terminal.read`, `terminal.write`, and `terminal.kill`.
- Web terminal UI no longer stores or submits a separate terminal token.
- TUI no longer creates share URLs; share links are created from Web
  Profile/Access where an authenticated user context exists.

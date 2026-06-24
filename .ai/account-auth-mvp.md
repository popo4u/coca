# Account/Auth MVP Implementation Tracker

## Goal

Implement the Account/auth surfaces described in `.ai/gap.md` as a local account MVP:
email/password signup and login, profile editing, device sessions, access token creation,
and token revocation.

## Architecture

- Browser talks to gateway APIs only.
- Gateway validates legacy share tokens or daemon-backed auth tokens, then forwards account
  work to daemon RPC.
- Daemon is the authoritative service host.
- `coca-app` owns account/auth workflows and API DTO behavior.
- `coca-core` owns SQLite persistence primitives.
- Terminal write actions keep the separate terminal token requirement.

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

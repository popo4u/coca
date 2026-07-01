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

# GUI and Web Sharing v1 Plan

## Goal

Keep `coca` useful as a local and remote read-only session browser while moving the architecture toward multiple frontends. The current terminal UI should keep working, and future GUI/Web frontends should consume the same core capabilities through an RPC boundary instead of duplicating provider parsing, settings, share URL, or launch planning logic.

The shared browser view remains read-only. It must not mutate provider histories, start resume/fork actions, or expose write-capable controls.

## Current Architecture

- `coca` is now a Rust workspace.
- `coca-core` owns normalized models, provider loaders, session catalog, settings, share rendering, remote loading, launch planning, and frontend-facing use cases.
- `coca-tui` owns terminal UI state, key handling, rendering, and the frontend `CoreClient` contract.
- `coca-daemon` hosts the core RPC router and server adapters.
- `coca-protocol` owns JSON-RPC wire types.
- `coca-ipc` owns local IPC framing and Unix socket helpers.
- The root `coca` binary owns CLI parsing, the frontend RPC client adapter, and the final process execution bridge.

## Implemented Baseline

- Branch `gui` contains the original read-only sharing work plus the core/frontend split.
- `coca core` supersedes the old separate `share serve` and `client serve` shape.
- `coca core` serves both:
  - `GET /api/sessions` for remote browsing.
  - `/s/<provider>/<session-id>?token=<token>` for read-only Web share pages.
- Historical settings included `core.bind`, configured remotes, origin visibility, launch defaults, `share.base_url`, and generated `share.token`; current auth uses account-scoped tokens and per-link share tokens instead.
- The TUI settings page can edit origin visibility, core bind, share settings, and launch defaults.
- Historically the TUI `u` key showed a read-only share URL for local sessions; current sharing is managed from Web Profile/Access with authenticated per-link tokens.
- The default TUI path uses the JSON-RPC core router through an in-process client, so terminal UI behavior follows the same frontend/core boundary as the local daemon.
- `coca daemon --socket <path>` exposes the JSON-RPC core boundary over local IPC for future GUI work.

## v1 Product Decisions

- Multiple frontends are expected. TUI, GUI, and future Web frontend code should not parse provider histories directly.
- Core owns provider parsing, session catalog, settings persistence, share URL generation, launch planning, and future process/PTY lifecycle.
- Frontends own presentation state, interaction state, and rendering.
- Current remote sessions remain browse-only. Remote launch, remote share forwarding, and remote PTY are deferred.
- PTY/PTS is not part of this v1 implementation, but architecture should keep process lifecycle out of TUI-specific code.

## Web Sharing Behavior

- Single-session URL format:

```text
<share.base_url>/s/<provider>/<encoded-session-id>?token=<secret>
```

- Web pages expose metadata and reconstructed transcript only.
- Web pages must not expose `source_path`, resume/fork controls, or write-capable actions.
- Dynamic content is HTML-escaped.
- Invalid token returns `401`.
- Unknown provider/session returns `404`.
- Unsupported methods return `405`.
- Share pages should stay compatible with local network direct access.

## Rendering v1 Baseline

- Current rendering is a first baseline, not a final Codex/Claude-like transcript experience.
- The page has a main transcript column and a sticky desktop metadata rail.
- The title panel is aligned with the transcript width and clamps long titles with the full title in the HTML `title` attribute.
- The first prompt is rendered as its own panel, and the duplicate first user transcript item is omitted when it exactly matches the prompt.
- Semantic transcript rendering classifies provider context wrappers and subagent notifications as structured blocks.

Next rendering work should improve role treatment, tool/action blocks, markdown/code presentation, spacing hierarchy, and mobile review.

## Implementation History

- `70aed1f feat: add read-only session sharing`
  - Added the original read-only Web share and TUI share URL behavior.
- `cda15f1 Refactor coca into core and frontend workspace`
  - Split the repo into core, TUI, daemon, protocol, and IPC crates.
- `d4c00ad Route TUI through core RPC client`
  - Routed the TUI through the core RPC client boundary and added the in-process JSON-RPC adapter.

## Verification

- Full workspace verification command:

```sh
cargo xtask verify
```

- The latest implementation was verified with `cargo xtask verify`.

## Follow-Up Work

- Replace the default in-process TUI transport with a socket-backed client when daemon lifecycle management is ready.
- Add GUI frontend on top of the same `CoreClient`/JSON-RPC boundary.
- Add version/compatibility checks around external GUI/TUI-to-daemon protocol usage.
- Continue improving browser transcript rendering toward the original provider app experience.
- Design PTY/PTS lifecycle in core before adding any frontend terminal embedding.

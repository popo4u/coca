# coca

`coca` (Chat Once, Continue Anywhere) is a unified terminal UI for local and configured remote coder-agent sessions.

It lets you browse, inspect, resume, and fork conversations created by tools like Codex and Claude from one place. Instead of remembering which agent owns a session or manually searching through provider-specific history files, `coca` normalizes local and remote histories into a single interactive session list.

## What It Does

- Lists local and configured remote Codex and Claude sessions in one TUI.
- Filters by provider and searches across session text.
- Shows session metadata and the full first prompt inline.
- Opens a transcript viewer for reconstructed conversation history.
- Shows a read-only React Web share URL for local sessions.
- Resumes existing sessions with the right provider command.
- Forks or executes sessions with provider-specific launch options.
- Fetches remote sessions through a read-only HTTP core.
- Keeps provider history read-only.

## Why

Coder agents are useful, but their conversations are fragmented across different CLIs, storage layouts, and resume commands. `coca` is a small manager for that growing local history:

- Find the conversation you want without switching tools.
- Review context before resuming.
- Resume or fork with fewer command-line details to remember.
- Add more providers without rewriting the TUI.

## Supported Providers

| Provider | Browse | Transcript | Resume | Fork |
| --- | --- | --- | --- | --- |
| Codex | Yes | Yes | `codex resume` | `codex fork` |
| Claude | Yes | Yes | `claude --resume` | `claude --resume --fork-session` |

## Usage

Run `coca` in a terminal. By default it reads sessions from:

- `~/.codex`
- `~/.claude`

Useful options:

```sh
coca --provider all
coca --provider codex
coca --provider claude
coca --codex-home ~/.codex --claude-home ~/.claude
coca --remote-config ~/.config/coca/remotes.json
```

By default `coca` reads and writes settings at `~/.config/coca/settings.json`:

```json
{
  "core": {
    "bind": "0.0.0.0:8787"
  },
  "remotes": [
    {
      "name": "work-mac",
      "base_url": "http://192.168.1.20:8787",
      "token": "<secret>",
      "enabled": true
    }
  ],
  "origin_visibility": {
    "local": true,
    "remotes": {
      "work-mac": true
    }
  },
  "launch_defaults": {
    "resume": { "use_current_dir": false, "yolo": false },
    "fork": { "use_current_dir": false, "yolo": false }
  },
  "share": {
    "base_url": "http://192.168.1.20:8787",
    "token": "<secret>"
  }
}
```

`share.token` is generated automatically when settings are first created or loaded without a token. Press `,` in the TUI to edit visible origins, `core.bind`, share settings, and the default launch options used by `s` execute and `f` fork dialogs. If `settings.json` does not exist, `coca` will still read an existing `~/.config/coca/remotes.json` for compatibility.

## Core

Run a read-only core on a machine that has Codex or Claude history:

```sh
coca core
```

The core listens on `core.bind` and serves the read-only remote session API. The default bind is `0.0.0.0:8787`. Browser pages are served by `coca web`, not by `coca core`.

## Web Frontend

Build the React frontend, then run the Web API/static host:

```sh
cd app/web
npm install
npm run build
cd ../..
coca web
coca web --bind 127.0.0.1:8787
```

The Web frontend uses `share.token` for API access:

```text
http://127.0.0.1:8787/?token=<secret>
```

Rust serves JSON APIs under `/api/v1/*` and static files from `app/web/dist`. React owns the browser UI for sessions, detail views, config overview, and share links. `/api/v1/stream` reserves the future terminal event transport shape.

Run the local JSON-RPC daemon for frontend/core IPC:

```sh
coca daemon
coca daemon --socket ~/.config/coca/core.sock
```

The daemon uses the same app-layer capabilities as the TUI and is the local IPC boundary intended for future GUI integration.

The default TUI path uses the same JSON-RPC core router through an in-process client, so terminal UI behavior and daemon behavior stay on the same frontend/core boundary.

Press `u` on a local session in the TUI to show its browser URL:

```text
http://192.168.1.20:8787/?token=<secret>#/session/local/codex/<session-id>
```

Shared sessions are browse-only and expose metadata plus the reconstructed transcript through the React Web app. The Web API does not include source paths or resume/fork commands in session DTOs. Anyone with the URL, token, and network access can read the session, so use a strong token and bind only to networks you trust.

Configure the browsing machine with `~/.config/coca/settings.json`:

```json
{
  "remotes": [
    { "name": "work-mac", "base_url": "http://192.168.1.20:8787", "token": "<secret>", "enabled": true }
  ]
}
```

Remote sessions support listing, search, details, and transcript viewing. Resume, execute, and fork are local-only in this version.

## Keybindings

| Key | Action |
| --- | --- |
| `Up` / `Down`, `j` / `k` | Move selection |
| `/` | Search sessions |
| `Tab` | Cycle provider filter |
| `,` | Open settings and edit saved configuration |
| `?` | Open help |
| `Space` | Expand or collapse session details |
| `t` | Open transcript viewer |
| `h` / `l` | Page transcript backward or forward |
| `u` | Show read-only share URL for local session |
| `Enter` | Resume selected session |
| `s` | Execute selected session with launch options |
| `f` | Fork selected session with launch options |
| `Esc`, `q` | Close modal or quit |

## Platform Support

`coca` is intended to run on Linux, macOS, and Windows.

Release artifacts are platform-specific. A macOS binary cannot run on Linux, and a Linux binary cannot run on Windows. Pick the artifact that matches your operating system and CPU architecture.

## Development

Development setup, local run commands, verification, and release build commands live in [docs/dev.md](docs/dev.md).

## Project Shape

The codebase is a Rust workspace that keeps the main responsibilities separated:

- `coca-core`: shared models, provider parsing, session catalog primitives, settings persistence primitives, remote loading, and launch construction primitives
- `coca-app`: user-visible use cases and frontend/API DTOs
- `coca-protocol`: JSON-RPC wire contract for frontends and core
- `coca-ipc`: local IPC framing and transport helpers
- `coca-daemon`: core host and server/RPC adapters
- `coca-tui`: terminal UI state, events, rendering, view helpers, and the frontend `CoreClient` contract
- `coca-web`: JSON API and static host for the browser frontend
- `app/web`: React + TypeScript Web frontend
- `app/gui`: reserved for a future desktop GUI frontend
- `app/tui`: reserved for a possible future terminal frontend location; current TUI code remains in `crates/coca-tui`
- root `coca`: CLI shell, frontend RPC client adapter, and platform-aware process execution bridge
- `xtask`: project automation for verification and builds

## Status

`coca` is early-stage software. The current focus is local session management for Codex and Claude, with an architecture designed to support more coder agents over time.

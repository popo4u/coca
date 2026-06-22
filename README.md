# coca

`coca` (Chat Once, Continue Anywhere) is a unified terminal UI for local and configured remote coder-agent sessions.

It lets you browse, inspect, resume, and fork conversations created by tools like Codex and Claude from one place. Instead of remembering which agent owns a session or manually searching through provider-specific history files, `coca` normalizes local and remote histories into a single interactive session list.

## What It Does

- Lists local and configured remote Codex and Claude sessions in one TUI.
- Filters by provider and searches across session text.
- Shows session metadata and the full first prompt inline.
- Opens a transcript viewer for reconstructed conversation history.
- Shows a read-only browser share URL for local sessions.
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

By default `coca` reads and writes settings at `~/.config/coca/settings.json`. Press `,` in the TUI to edit visible origins, core bind settings, share settings, and launch defaults. `share.token` is generated automatically when settings are first created or loaded without a token.

## Core

Run a read-only core on a machine that has Codex or Claude history:

```sh
coca core
```

The core listens on `core.bind` and serves the read-only remote session API. The default bind is `0.0.0.0:8787`. Browser pages are served by `coca web`, not by `coca core`.

## Web Frontend

Build the Web frontend, then run the Web host:

```sh
cd app/web
npm install
npm run build
cd ../..
coca web
coca web --bind 127.0.0.1:8787
```

Open the printed URL, or pass `share.token` once as a query token:

```text
http://127.0.0.1:8787/?token=<secret>
```

Run the local daemon for advanced integrations:

```sh
coca daemon
coca daemon --socket ~/.config/coca/core.sock
```

Press `u` on a local session in the TUI to show its browser URL:

```text
http://192.168.1.20:8787/?token=<secret>#/session/local/codex/<session-id>
```

Shared sessions are browse-only. Anyone with the URL, token, and network access can read the session, so use a strong token and bind only to networks you trust.

Configure the browsing machine with `~/.config/coca/settings.json`:

```json
{
  "remotes": [
    { "name": "work-mac", "base_url": "http://192.168.1.20:8787", "token": "<secret>", "enabled": true }
  ]
}
```

Remote sessions support listing, search, details, and transcript viewing. Resume, execute, and fork are local-only in this version.

## Architecture

```text
                    +-----------+
                    | src/ CLI  |
                    +-----+-----+
                          |
        +-----------------+-----------------+
        |                 |                 |
        v                 v                 v
+---------------+ +---------------+ +---------------+
| UI adapter    | | HTTP adapter  | | IPC/RPC host  |
|               | |               | |               |
| coca-tui      | | coca-web      | | coca-daemon   |
+-------+-------+ +-------+-------+ +-------+-------+
        |                 |                 |
        +-----------------+-----------------+
                          |
                          v
                    +------------+
                    | coca-app   |
                    +-----+------+
                          |
                          v
                    +------------+
                    | coca-core  |
                    +-----+------+
                          |
        +-----------------+-----------------+
        |                 |                 |
        v                 v                 v
   ~/.codex          ~/.claude        remote cores

```

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

## Status

`coca` is early-stage software. The current focus is local session management for Codex and Claude, with an architecture designed to support more coder agents over time.

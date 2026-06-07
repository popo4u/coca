# coca

`coca` (Chat Once, Continue Anywhere) is a unified terminal UI for local coder-agent sessions.

It lets you browse, inspect, resume, and fork conversations created by tools like Codex and Claude from one place. Instead of remembering which agent owns a session or manually searching through provider-specific history files, `coca` normalizes the local histories into a single interactive session list.

## What It Does

- Lists local Codex and Claude sessions in one TUI.
- Filters by provider and searches across session text.
- Shows session metadata and the full first prompt inline.
- Opens a transcript viewer for reconstructed conversation history.
- Resumes existing sessions with the right provider command.
- Forks or executes sessions with provider-specific launch options.
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
```

## Keybindings

| Key | Action |
| --- | --- |
| `Up` / `Down`, `j` / `k` | Move selection |
| `/` | Search sessions |
| `Tab` | Cycle provider filter |
| `Space` | Expand or collapse session details |
| `t` | Open transcript viewer |
| `h` / `l` | Page transcript backward or forward |
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

The codebase keeps the main responsibilities separated:

- `model`: shared normalized session types
- `providers`: read-only provider history parsing
- `launch`: provider-specific resume and fork command construction
- `process`: platform-aware process execution
- `tui`: app state, events, rendering, and view helpers
- `xtask`: project automation for verification and builds

## Status

`coca` is early-stage software. The current focus is local session management for Codex and Claude, with an architecture designed to support more coder agents over time.

# Development

This guide covers local setup, running the app from source, verification, and build commands.

## Prerequisites

- Rust stable toolchain
- Cargo
- Node.js and npm for the React Web frontend
- A terminal that supports TUI applications
- Optional: Codex and/or Claude session history on the machine

The app reads provider histories from `~/.codex` and `~/.claude` by default. Provider history files are read-only inputs.

## Run Locally

From the repository root:

```sh
cargo run -- --provider all
```

Provider filters:

```sh
cargo run -- --provider codex
cargo run -- --provider claude
```

Override provider history roots:

```sh
cargo run -- --codex-home ~/.codex --claude-home ~/.claude
```

Use a remote config:

```sh
cargo run -- --remote-config ~/.config/coca/remotes.json
```

The default persistent settings file is:

```sh
~/.config/coca/settings.json
```

It stores daemon socket settings, gateway bind settings, configured remotes,
origin visibility, share settings, terminal settings, and the default launch
options used by the `s` execute and `f` fork dialogs. The TUI settings page is
opened with `,`. `--remote-config` remains available as a remotes-only
override, and an existing `~/.config/coca/remotes.json` is still read when
`settings.json` does not exist.

Recommended local service management:

```sh
cargo xtask run
cargo xtask dev status
cargo xtask dev logs
cargo xtask dev stop
```

`cargo xtask run` builds the native debug binary for the current OS, stops old
managed services, and starts the local daemon, gateway, and Vite dev server. It
stores PID files and logs under `.ai/run/xtask-dev/`, binds HTTP services on
`0.0.0.0`, and prints local URLs such as `http://127.0.0.1:5173`. By default it
uses port `8787` for the gateway and `5173` for Vite. If either port is occupied
by a non-coca process, use smart-port mode:

```sh
cargo xtask run --smart-port
```

Use release mode when you want to exercise the built binary and static Web
assets:

```sh
cargo xtask dev restart --mode release
```

Manual daemon startup is still useful for focused debugging:

```sh
cargo run -- daemon
cargo run -- daemon --socket ~/.config/coca/daemon.sock
```

Build and run the React Web frontend through the browser gateway manually:

```sh
cd app/web
npm install
npm run build
cd ../..
cargo run -- gateway --bind 0.0.0.0:8787
```

Open the Web frontend and sign in with a local account. The first account can be
created from the browser sign-up screen.

During frontend development, the xtask dev command runs Vite for you. To run it
manually and proxy API calls to `coca gateway`:

```sh
cd app/web
npm run dev
```

The browser app talks to `coca gateway`. Gateway proxies business and terminal
runtime APIs to `coca daemon`; it does not own provider parsing or terminal
lifecycle. The TUI follows the same authority boundary through a daemon client.

In the TUI, press `,` to edit gateway, share base URL, terminal enablement, and
launch settings. Share links are managed from the Web Profile/Access flow after
signing in. Restart `coca gateway` after changing gateway bind or share settings
used by the running browser gateway.

Show CLI help:

```sh
cargo run -- --help
cargo run -- gateway --help
```

## Verification

The canonical full verification command is:

```sh
cargo xtask verify
```

It runs:

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The Rust verification command does not currently run frontend checks. Run these
after changing `app/web`:

```sh
cd app/web
npm run build
```

Focused commands are also available:

```sh
cargo xtask fmt
cargo xtask check
cargo xtask test
cargo xtask clippy
```

## Build Commands

Debug build:

```sh
cargo xtask build
```

Release build:

```sh
cargo xtask build --release
```

Build a release binary and copy it to `dist/`:

```sh
cargo xtask dist
```

Build for a specific target:

```sh
cargo xtask dist --target linux-x64
cargo xtask dist --target linux-arm64
cargo xtask dist --target macos-x64
cargo xtask dist --target macos-arm64
cargo xtask dist --target windows-x64
```

Print target aliases:

```sh
cargo xtask targets
```

Build all known Linux, macOS, and Windows release binaries:

```sh
cargo xtask dist-all
```

Cross-platform builds require the corresponding Rust targets and, for some targets, platform-specific linkers or SDKs. Install Rust targets with:

```sh
rustup target add x86_64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin
rustup target add x86_64-pc-windows-msvc
```

This project uses `rusqlite` with bundled SQLite, so Linux GNU targets also compile SQLite C code. On non-Linux hosts, `cargo xtask build --release --target linux-x64` and `cargo xtask dist --target linux-x64` use `cargo zigbuild` when both `cargo-zigbuild` and `zig` are installed. If they are unavailable and no Linux GNU C toolchain is configured, `xtask` tries Docker instead. Make sure Docker is installed and the daemon is running, or install a target C compiler such as `x86_64-linux-gnu-gcc` and expose it through `CC_x86_64_unknown_linux_gnu`.

## Platform-Specific Binaries

Each binary in `dist/` is built for one platform and architecture. For example:

- `coca-macos-x64` runs on Intel macOS.
- `coca-macos-arm64` runs on Apple Silicon macOS.
- `coca-linux-x64` runs on x86_64 Linux.
- `coca-linux-arm64` runs on ARM64 Linux.
- `coca-windows-x64.exe` runs on x86_64 Windows.

If Linux reports `cannot execute binary file: Exec format error`, the binary was built for a different operating system or CPU architecture. Build or download the matching target instead.

## GitHub CI

GitHub Actions runs `cargo xtask verify` on pull requests and pushes, then builds downloadable release binaries for:

- `coca-linux-x64`
- `coca-windows-x64.exe`
- `coca-macos-x64`
- `coca-macos-arm64`

The current release flow intentionally publishes bare binaries only. It does not create archives, installers, checksums, signatures, or notarized artifacts. Tag pushes publish those binaries as GitHub Release assets.

## Architecture Notes

The workspace is organized by responsibility:

- `crates/coca-core/`: normalized models, provider loaders, session catalog primitives, settings persistence primitives, remote loading, and launch construction primitives.
- `crates/coca-app/`: app-layer use cases and frontend/API DTOs.
- `crates/coca-protocol/`: JSON-RPC wire types for frontend/daemon communication.
- `crates/coca-ipc/`: local IPC framing and transport helpers.
- `crates/coca-daemon/`: local authoritative service host, RPC router, terminal runtime, and server adapters.
- `crates/coca-tui/`: app state, key handling, rendering, view helpers, and the frontend daemon-client contract.
- `crates/coca-web/`: browser gateway host for HTTP APIs, WebSocket bridges, and static assets.
- `app/web/`: React + TypeScript browser frontend that talks to the gateway.
- `app/gui/`: reserved for a future desktop GUI frontend.
- `app/tui/`: reserved for a possible future terminal frontend location; current TUI code remains in `crates/coca-tui/`.
- `src/`: root CLI shell, frontend RPC client adapter, and final process execution bridge.
- `xtask/`: project automation.

When adding a provider:

1. Add provider-specific parsing under `crates/coca-core/src/providers/`.
2. Normalize data into `coca_core::model::Session`.
3. Add provider command construction in `crates/coca-core/src/launch.rs`.
4. Keep provider storage read-only.
5. Add focused parser and launch tests.

## Repository Checks Before Publishing

Before publishing a release or opening a pull request:

```sh
cargo xtask verify
```

Then build the platform binary you need:

```sh
cargo xtask dist --target <target-alias-or-triple>
```

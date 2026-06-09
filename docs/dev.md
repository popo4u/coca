# Development

This guide covers local setup, running the app from source, verification, and build commands.

## Prerequisites

- Rust stable toolchain
- Cargo
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

Show CLI help:

```sh
cargo run -- --help
```

## Verification

The canonical full verification command is:

```sh
cargo xtask verify
```

It runs:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
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

The crate is organized by responsibility:

- `src/model.rs`: normalized provider/session data.
- `src/providers/`: read-only loaders for provider history.
- `src/launch.rs`: resume, execute, and fork command construction.
- `src/process.rs`: Unix `exec` and non-Unix child-process fallback.
- `src/cli.rs`: command-line parsing and default provider roots.
- `src/tui/`: app state, key handling, rendering, and view helpers.
- `xtask/`: project automation.

When adding a provider:

1. Add provider-specific parsing under `src/providers/`.
2. Normalize data into `model::Session`.
3. Add provider command construction in `src/launch.rs`.
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

# coca LLM Deploy Runbook

This runbook is for agents that need to build and deploy the current `coca`
workspace. It intentionally describes the process instead of encoding one fixed
machine topology in code.

The deploy agent must adapt to the current host, target architecture, configured
remotes, and service manager by inspecting the environment first. Do not assume
that a previous deploy target, host name, port, architecture, or service manager
is still correct.

## Goal

- Build the latest `coca` binary from the current workspace.
- Update the local `coca` executable and restart the local `coca daemon` and
  `coca gateway` processes that are actually managed on this host.
- Update only the remote environments explicitly requested or confirmed by the
  user, then restart their `coca daemon` and `coca gateway` processes.
- Verify that local sessions, remote sessions, and the local browser share page
  are usable after deployment.

## Hard Rules

- Read `AGENTS.md`, `docs/architecture-and-style.md`, and `docs/dev.md` before
  changing anything.
- Check `git status --short` before deploying. Do not revert or overwrite user
  work.
- Do not print tokens, full share URLs with tokens, or sensitive config values.
  Redact tokens in all logs and summaries.
- If the user did not specify a remote target, do not guess. Inspect configured
  remotes, then ask the user which remote, if any, should be deployed.
- Historical targets such as a previous host, previous PID, or previous remote
  name are not authorization to deploy that remote again.
- Prefer a subagent for remote deploy work when subagents are available, so the
  main context stays focused.
- Build artifacts must match the target OS and CPU architecture before they are
  installed.
- If service management cannot be determined safely, stop and ask the user
  instead of improvising.

## Remote Selection

Remote deployment requires explicit user intent.

If the user names a remote host or configured remote name, deploy only that
remote after verifying it exists or is reachable.

If the user does not name a remote:

1. Read `~/.config/coca/settings.json`.
2. Summarize enabled remotes by name and redacted base URL.
3. Ask the user to choose one of:
   - local only
   - one specific enabled remote
   - all enabled remotes
4. Do not deploy any remote until the user answers.

If there are no enabled remotes, deploy local only and say that no enabled
remote was found.

## Discovery Phase

Gather facts before executing deployment steps.

Local checks:

- Current workspace and repository status.
- Host OS and CPU architecture, for example `uname -s` and `uname -m`.
- Current installed binary path, usually discovered with `command -v coca`.
- Current installed binary version with `coca --version`, if available.
- Runtime settings from `~/.config/coca/settings.json`, with tokens redacted.
- Local `daemon.socket`, `daemon.terminal_socket`, and `gateway.bind` settings,
  not values remembered from prior deployments.
- Current `coca daemon` PID and socket paths.
- Current `coca gateway` PID and listening address.
- Service manager, for example `launchctl`, `systemd`, or a manually started
  process.

Remote checks, only after the user has selected the remote targets:

- SSH reachability.
- Remote OS and CPU architecture.
- Remote installed binary path and version.
- Remote `daemon.socket`, `daemon.terminal_socket`, and `gateway.bind`
  settings, with tokens redacted.
- Remote `coca daemon` PID and socket paths.
- Remote `coca gateway` PID and listening address.
- Remote service manager, if any.
- Remote log paths, if discoverable.

## Build Phase

Use the existing project build flow.

Local release build:

```sh
cargo xtask build --release
```

Remote release build:

```sh
cargo xtask dist --target <target-alias>
```

Choose `<target-alias>` from the discovered remote OS and CPU architecture, for
example:

- `linux-x64` for Linux x86_64
- `linux-arm64` for Linux arm64 or aarch64
- `macos-x64` for macOS x86_64
- `macos-arm64` for macOS arm64

After building, inspect the artifact with `file` and compare it with the target
system. Do not install a binary if the platform or architecture does not match.

## Local Deploy Phase

1. Install the release binary to the discovered local install path.
2. Restart local `coca daemon` using the discovered service manager.
3. Restart local `coca gateway` using the discovered service manager when a
   gateway is configured or currently running.
4. If either process was manually started, stop only the exact matching process
   and restart it with equivalent socket/bind/settings behavior.
5. Verify the new daemon PID, gateway PID, daemon sockets, and gateway listening
   address.

Common macOS LaunchAgent flow, when discovered:

```sh
install -m 0755 target/release/coca /usr/local/bin/coca
launchctl kickstart -k gui/$UID/<launch-label>
```

This is an example, not a default. Use it only when the current environment
actually uses LaunchAgents for `coca daemon` and/or `coca gateway`.

## Remote Deploy Phase

Remote deploy should be delegated to a subagent when available. The subagent
must receive only the selected remote targets and must not print tokens.

For each selected remote:

1. Upload the matching artifact to a temporary path.
2. Install it to the discovered remote install path with mode `0755`.
3. Stop only exact old `coca daemon` and `coca gateway` processes that belong to
   the selected remote target. Do not kill broad matches that could include SSH
   shells or unrelated commands.
4. Restart `coca daemon` and `coca gateway` with the discovered service manager.
5. If no service manager exists, start them in the background and redirect logs
   to stable locations under `~/.local/state/coca/` when possible.
6. Verify the remote daemon PID, gateway PID, daemon sockets, and gateway
   listening address over SSH.

Exact process matching should prefer patterns equivalent to:

```sh
pgrep -af '^/usr/local/bin/coca daemon( |$)'
pgrep -af '^/usr/local/bin/coca gateway( |$)'
```

Adjust the path if the discovered remote binary path is different.

## Verification Phase

Local verification:

- `coca --version` runs from the installed path.
- `coca daemon` has one expected PID and its configured sockets are available.
- `coca gateway` has one expected PID when configured or expected for browser
  access.
- The configured `gateway.bind` address is listening when gateway is expected.
- `GET <share.base_url>/api/v1/sessions` returns HTTP 200 with the configured
  share token.
- If local sessions exist, one generated `/s/<provider>/<session-id>` share page
  returns HTTP 200. Print only a redacted URL.

Remote verification, for each selected remote:

- SSH `coca --version` runs from the installed remote path.
- Remote `coca daemon` has one expected PID and its configured sockets are
  available.
- Remote `coca gateway` has one expected PID when configured or expected for
  browser/terminal access.
- The remote `gateway.bind` address is listening when gateway is expected.
- From the local machine, `GET <remote.base_url>/api/v1/sessions` returns HTTP
  200 with that remote's configured token.

Final summary must include:

- Local install path, daemon PID, gateway PID, daemon socket paths, gateway
  listening address, API HTTP status, session count, and share page HTTP status.
- Each remote name, host, install path, daemon PID, gateway PID, gateway
  listening address, API HTTP status, and session count.
- Any failed phase with the failing command and concise stderr summary.
- No tokens.

## Failure Handling

- If build fails, stop. Do not deploy old artifacts.
- If target architecture cannot be determined, stop and ask.
- If remote target was not specified or confirmed, do not deploy remote.
- If upload fails, stop before changing the remote install path.
- If install succeeds but restart fails, report the service status and log
  paths.
- If API verification fails, report the redacted URL, HTTP status, and relevant
  daemon/gateway log summary.
- If multiple `coca daemon` or `coca gateway` processes are found, handle only
  exact matches. Ask the user before killing ambiguous processes.

## Prompt Template

Use this when asking an agent or subagent to run the deploy:

```text
Please deploy the current coca workspace using .ai/deploy-runbook.md.

Requirements:
1. Inspect the current local environment before executing deploy steps.
2. If I did not explicitly specify a remote, read configured enabled remotes and
   ask me which remote, if any, to deploy. Do not guess.
3. Read ~/.config/coca/settings.json for base URLs and tokens, but never print
   tokens.
4. Build the latest local binary and any selected remote binary for the matching
   target architecture.
5. Replace the local binary, restart local coca daemon and coca gateway, and
   verify local API and share page.
6. For selected remotes only, upload the matching artifact, replace the remote
   binary, restart remote coca daemon and coca gateway, and verify remote API.
7. Prefer subagents for remote deploy work when available.
8. Final output should summarize daemon PID, gateway PID, listening address,
   HTTP status, session count, and failures only. Do not include tokens.
```

# Web Redesign Gaps

This note records parts of the root `index.html` prototype that are not backed by the current gateway/daemon APIs. It does not override `AGENTS.md`, `docs/architecture-and-style.md`, or explicit user instructions.

## Implemented Against Existing APIs

- Workspace shell, dense tables, transcript timeline, runtime readiness copy, theme/accent/density preferences, and responsive layout are frontend-only.
- Sessions, detail, share links, config summary, health, remotes, terminal registry, and terminal WebSocket behavior use existing gateway APIs.
- Terminal controls preserve the current model: browser UI requests actions through gateway; daemon owns terminal lifecycle.
- Active terminal attach now routes to a dedicated live runtime page at `#/terminal/:origin/:provider/:sessionId/:terminalId`. The older `#/session/...?...terminal=` form is kept as a compatibility entry point and resolves to the live page.
- The live terminal page follows the prototype's terminal workspace shape with a full-width xterm area, runtime context rail, transcript/registry links, and a maximize mode that gives the terminal the viewport.
- Non-dashboard operational pages now use the full workspace width instead of residual centered max-width stacks.

## Gaps

- **Account/auth surfaces:** the prototype includes email/password, signup, SSO, profile editing, device sessions, token creation, and token revocation. Current web supports only local bearer-token entry plus separate terminal-token storage.
- **Origin fleet management:** the prototype includes machine IP, CPU, memory, version, install, update, and uninstall actions. Current API exposes only configured remote summaries.
- **Dashboard persistence:** pinned sessions, recent activity, audit feed, and default landing preferences have no durable API.
- **Terminal registry richness:** current terminal summaries expose id, session ref, mode, state, attached clients, writer, seq, size, and exit. The prototype terminal table/live rail expects command, title, provider/cwd joins, disconnected labels, and richer derived state.
- **Global search:** the prototype implies cross-surface search over sessions, cwd, model, and terminal ids. Current implementation can only filter loaded client-side data.
- **State vocabulary:** the prototype names more states than the current typed terminal state union. The UI derives badge copy from terminal capability errors, config fields, and WebSocket/API errors until a stable backend vocabulary exists.
- **Attach progress events:** the prototype shows staged browser/gateway/daemon progress. Current WebSocket API exposes terminal opened/output/exit/error events, so the React overlay can show the requested attach handoff but cannot prove each backend stage without adding explicit progress events.

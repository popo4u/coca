# Web Redesign Completion Plan

This note tracks the current implementation status for aligning `app/web` with the root `index.html` prototype. It does not override `AGENTS.md` or `docs/architecture-and-style.md`.

## Current Goal

1. Change: route active terminal attach into a dedicated live terminal workspace.
   Verify: dashboard and Active Terminals attach links resolve to `#/terminal/:origin/:provider/:sessionId/:terminalId`; legacy `#/session/...?...terminal=` still opens the live page.

2. Change: keep session detail focused on transcript and metadata while preserving terminal resume/fork controls in a full-width panel.
   Verify: session detail no longer renders attached terminal streams in a narrow right rail.

3. Change: maximize live/detail terminal panels so sidebars and secondary controls stop competing with xterm space.
   Verify: maximize hides terminal access/actions/list rails and gives the terminal surface the viewport.

4. Change: remove residual centered max-width constraints from operational pages.
   Verify: sessions, origins, terminals, profile, settings, and detail use the available workspace width.

## Remaining Product/API Gaps

See `.ai/gap.md` for backend/API gaps that cannot be completed purely in the React client, especially account/auth APIs, origin fleet telemetry, terminal registry richness, global search, and explicit attach-progress events.

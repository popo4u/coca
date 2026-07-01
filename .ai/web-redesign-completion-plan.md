# Web Redesign Completion Plan

This note tracks the current implementation status for aligning `app/web` with the root `index.html` prototype. It does not override `AGENTS.md` or `docs/architecture-and-style.md`.

## Current Goal

1. Change: keep authentication on the prototype's single-card login/signup surface while using real account APIs.
   Verify: sign-in and first-account forms share the centered card, remember-me controls token storage, and no fake `transcript.read` scope remains.

2. Change: route active terminal attach into a dedicated live terminal workspace.
   Verify: dashboard, session rows, detail related-terminal links, and Active Terminals attach links resolve to `#/terminal/:origin/:provider/:sessionId/:terminalId`; legacy `#/session/...?...terminal=` still opens the live page.

3. Change: keep session detail focused on transcript and metadata while preserving factual runtime controls.
   Verify: session detail no longer loads or renders xterm; Resume/Fork route to live terminal only after a real `terminal.opened`; related terminal links come from `/api/v1/terminal/sessions`.

4. Change: maximize live terminal panels so sidebars and secondary controls stop competing with xterm space.
   Verify: maximize hides secondary controls and gives the terminal surface the viewport.

5. Change: remove residual centered max-width constraints from operational pages.
   Verify: sessions, origins, terminals, profile, settings, and detail use the available workspace width.

## Remaining Product/API Gaps

See `.ai/gap.md` for backend/API gaps that cannot be completed purely in the React client, especially origin fleet telemetry, durable dashboard activity/pins, dedicated session-terminal APIs, terminal registry richness, global search, transcript-to-terminal sequence links, and explicit attach-progress events.

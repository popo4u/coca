# AGENTS.md

Guidance for agents working in this repository.

## Start Here

`coca` is a Rust workspace for browsing, inspecting, resuming, and forking local coder-agent sessions. It supports Codex and Claude today and should stay easy to extend to new providers and frontends.

Before making code changes, read [docs/architecture-and-style.md](docs/architecture-and-style.md). That file owns this project's architecture and style constraints.

Use [docs/dev.md](docs/dev.md) for local setup, run commands, verification, and release builds.

## Long-Term Task Memory

Use `.ai/` for durable planning notes, long-term task memory, and handoff context. Files in `.ai/` can record goals, staged plans, product decisions, and implementation status for work that spans multiple sessions.

`AGENTS.md` remains the constitution-level repository guidance: architecture constraints and working principles. Do not treat `.ai/` notes as overriding `AGENTS.md`, `docs/architecture-and-style.md`, or explicit user instructions.

## Deployment Runbook

For LLM-driven build, update, or deploy work, use `.ai/deploy-runbook.md` as the process runbook. Inspect the current environment first, build artifacts that match the target OS/architecture, restart the discovered `coca core` service, and verify sessions API/share pages.

Remote deployment requires explicit user intent. If the user does not name a remote, inspect enabled remotes in `~/.config/coca/settings.json`, summarize them with secrets redacted, and ask before deploying any remote. Do not infer remote targets from prior sessions. Redact tokens in all output and prefer subagents for remote deploy work when available.

## Working Principles

- State important assumptions before coding when the request has multiple plausible interpretations.
- Ask when ambiguity would risk the wrong behavior; otherwise make the smallest reasonable assumption and keep moving.
- For broad refactors or multi-file work, follow the main-session coordination model below.
- Prefer the minimum code that solves the requested problem. Do not add speculative features, one-off abstractions, or configurability that was not requested.
- Keep edits surgical. Touch only files and lines that directly support the task.
- Match the existing style and module boundaries, even when a different structure would also work.
- Clean up unused code introduced by your own changes. Mention unrelated cleanup opportunities instead of performing them.

## Main Session Role

For non-trivial work, treat the main session as the coordinator and acceptance owner. The main session should define the goal, choose task boundaries, check progress, integrate results, and perform final verification.

Keep the main session focused on decisions, cross-cutting context, and acceptance criteria. Avoid loading broad implementation detail into the main context when a subagent or another dynamic workflow can inspect a narrow area and report back.

Use subagents or other dynamic workflows as the default best practice for broad refactors, multi-file investigations, parallel verification, remote deploy work, and independent Rust/Web/docs tracks. Give each delegated task a narrow, non-overlapping ownership area, explicit deliverables, and expected verification.

The main session remains accountable for the result. It should reconcile delegated findings, avoid duplicating delegated implementation work in the main thread, run or inspect final verification, and produce the handoff summary.

## Goal-Driven Changes

For non-trivial work, translate the request into a short verifiable goal before editing:

```text
1. Change: [what will be updated]
   Verify: [focused command or inspection]
2. Change: [what will be updated]
   Verify: [focused command or inspection]
```

When fixing a bug, prefer a focused failing test or reproduction first, then make it pass. When refactoring, preserve behavior and use tests or targeted inspections to prove that the behavior stayed intact.

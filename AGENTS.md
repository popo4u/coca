# AGENTS.md

Guidance for agents working in this repository.

## Start Here

`coca` is a Rust terminal UI for browsing, inspecting, resuming, and forking local coder-agent sessions. It supports Codex and Claude today and should stay easy to extend to new providers.

Before making code changes, read [docs/architecture-and-style.md](docs/architecture-and-style.md). That file owns this project's architecture and style constraints.

Use [docs/dev.md](docs/dev.md) for local setup, run commands, verification, and release builds.

## Long-Term Task Memory

Use `.ai/` for durable planning notes, long-term task memory, and handoff context. Files in `.ai/` can record goals, staged plans, product decisions, and implementation status for work that spans multiple sessions.

`AGENTS.md` remains the constitution-level repository guidance: architecture constraints, working principles, and verification expectations. Do not treat `.ai/` notes as overriding `AGENTS.md`, `docs/architecture-and-style.md`, or explicit user instructions.

## Working Principles

- State important assumptions before coding when the request has multiple plausible interpretations.
- Ask when ambiguity would risk the wrong behavior; otherwise make the smallest reasonable assumption and keep moving.
- Prefer the minimum code that solves the requested problem. Do not add speculative features, one-off abstractions, or configurability that was not requested.
- Keep edits surgical. Touch only files and lines that directly support the task.
- Match the existing style and module boundaries, even when a different structure would also work.
- Clean up unused code introduced by your own changes. Mention unrelated cleanup opportunities instead of performing them.

## Goal-Driven Changes

For non-trivial work, translate the request into a short verifiable goal before editing:

```text
1. Change: [what will be updated]
   Verify: [focused command or inspection]
2. Change: [what will be updated]
   Verify: [focused command or inspection]
```

When fixing a bug, prefer a focused failing test or reproduction first, then make it pass. When refactoring, preserve behavior and use tests or targeted inspections to prove that the behavior stayed intact.

## Verification

The canonical full verification command is:

```sh
cargo xtask verify
```

While implementing, run focused checks locally when they are useful:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

When subagents are available, delegate full `cargo xtask verify` to a verifier subagent after implementation. The verifier should report pass/fail and a concise failure summary only, so the main session context stays focused.

If subagents are unavailable, run `cargo xtask verify` locally.

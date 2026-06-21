# Product

## Register

product

## Users

`coca` is for developers who use coder agents across local and configured remote machines. They arrive with a concrete task: find an earlier Codex or Claude session, inspect enough context to trust it, then resume, fork, share, or verify configuration without manually spelunking provider-specific history.

The primary context is a work session at a terminal or browser on a developer workstation. Users are technical, time-sensitive, and often comparing similar conversations by provider, origin, working directory, model, prompt, and transcript content.

## Product Purpose

`coca` normalizes fragmented coder-agent history into one reliable session manager. Success means a developer can quickly answer: what happened, where did it happen, which agent owns it, can I safely share it, and what action should I take next.

The interface should make local and remote provenance obvious, keep provider history read-only, and preserve the same core/frontend boundary across TUI, Web, and future frontends.

## Brand Personality

Precise, calm, technical.

The product should feel like a trustworthy engineering instrument: quiet under load, explicit about state, and careful with sensitive tokens and shared transcript access. It can have a recognizable point of view, but that identity should never compete with scanning, comparison, or command execution.

## Anti-references

Avoid marketing SaaS dashboards, decorative AI-tool chrome, noisy terminal cosplay, mascot-driven whimsy, and generic "developer dark mode" surfaces that use glow, gradients, and oversized metrics as filler.

Avoid UI that obscures provenance, hides security implications, treats read-only sharing casually, or makes session comparison depend on color alone.

## Design Principles

1. Provenance first: provider, origin, recency, working directory, and access mode must be visible before decorative detail.
2. Dense, not cramped: users should be able to scan many sessions quickly while still reading long prompts and paths without layout breakage.
3. Read-only trust: sharing and remote browsing must visibly communicate scope, token use, and safety limits.
4. One core, many surfaces: TUI and Web should feel like siblings even when their visual languages differ.
5. Standard affordances: navigation, tables, config summaries, transcript reading, and status indicators should use familiar product patterns.

## Accessibility & Inclusion

Target WCAG AA for contrast and keyboard-first operation. Do not rely on color alone for provider, origin, warning, or enabled/disabled states. Preserve readable transcript typography, support reduced motion, and keep responsive layouts usable on narrow browser viewports.

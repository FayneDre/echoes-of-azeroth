# 0001. Tauri 2 + Svelte 5

- Status: Accepted
- Date: 2026-11-13

## Context

A public desktop launcher for Windows (Linux/macOS later). Players run the game
alongside it, so the launcher must stay light. The core logic (downloads,
database, processes) must be testable and reusable in CI and a CLI.

## Decision

- Tauri 2 shell (Rust, system WebView2), ~30–60 MB RAM vs 200–400 MB for Electron.
- Svelte 5 + TypeScript + Vite for the UI (small runtime, simple reactivity).
- Business logic in independent crates under `crates/`; `app/src-tauri` is a thin adapter.
- Dependency-free i18n (FR + EN from day one, English as fallback).

## Consequences

- Rust toolchain (MSVC) required for contributors.
- Same crates power the `eoa` CLI used by CI and support.
- Cross-platform later mostly means OS-specific bits behind traits (Job Objects, paths).

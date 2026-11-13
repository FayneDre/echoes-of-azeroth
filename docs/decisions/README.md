# Architecture Decision Records

One file per decision: `NNNN-short-title.md`, never rewritten once accepted.
A changed decision gets a new ADR that supersedes the old one.

| # | Decision | Status |
|---|---|---|
| [0001](0001-tauri-svelte.md) | Tauri 2 + Svelte 5, Rust core crates independent of Tauri | Accepted |
| [0002](0002-mysql-8-4-portable.md) | MySQL 8.4 LTS, portable, one instance per realm | Accepted |
| [0003](0003-full-build-modules.md) | One full build with all catalog modules, toggled at runtime | Accepted |
| [0004](0004-manifests-and-drivers.md) | Declarative manifests + per-emulator Rust drivers | Accepted |
| [0005](0005-distribution.md) | Open source, GitHub Releases + mirrors, SignPath signing | Accepted |

Template:

```markdown
# NNNN. Title
- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD
## Context
## Decision
## Consequences
```

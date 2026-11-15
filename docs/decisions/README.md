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
| [0006](0006-upstream-server-data.md) | Server data downloaded from pinned upstream release, never re-hosted | Accepted |
| [0007](0007-instance-config-in-manifest.md) | Instance config declared in the emulator manifest; instance layout | Accepted |
| [0008](0008-module-sql-when-disabled.md) | Module SQL applied while disabled, per manifest flag (`applyWhenDisabled`) | Accepted |
| [0009](0009-database-snapshot.md) | Database snapshot: cold copy of the MySQL data directory after base dbimport | Accepted |
| [0010](0010-signed-index.md) | Signed artifact index: one envelope file, ed25519, serial anti-rollback, selection by catalog pins | Accepted |
| [0011](0011-runtime-supervisor.md) | Runtime supervisor: stop methods declared per process, Job Object for stateless ones, accounts written by the driver (SRP6) | Accepted |
| [0012](0012-world-regeneration.md) | World regeneration on module removal: tables moved aside with one RENAME TABLE, rebuilt by dbimport | Accepted |
| [0013](0013-client-integration.md) | Client integration: managed client files, module assets downloaded from upstream, per-instance server data overlay | Accepted |
| [0014](0014-launcher-services.md) | Launcher services shared by the app and the CLI: `eoa-launcher`, error codes, progress and session events, pending changes, repair | Accepted |
| [0015](0015-launcher-distribution.md) | Launcher distribution: per-user NSIS installer, SignPath, signed auto-update feed, opt-in crash reports, zstd artifacts | Accepted |
| [0016](0016-backups-and-lan-join.md) | Player-data backups, retention, restore of the player databases, joining a LAN realm | Accepted |

Template:

```markdown
# NNNN. Title
- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD
## Context
## Decision
## Consequences
```

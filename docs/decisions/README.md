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
| [0014](0014-launcher-services.md) | Launcher services shared by the app and the CLI: `rk-launcher`, error codes, progress and session events, pending changes, repair | Accepted |
| [0015](0015-launcher-distribution.md) | Launcher distribution: per-user NSIS installer, SignPath, signed auto-update feed, opt-in crash reports, zstd artifacts | Accepted |
| [0016](0016-backups-and-lan-join.md) | Player-data backups, retention, restore of the player databases, joining a LAN realm | Accepted |
| [0017](0017-realm-settings-uninstall-location.md) | Realm settings (rates, realm type) as emulator data, uninstall, choosable install location | Accepted |
| [0018](0018-beta-readiness-player-comfort.md) | Free-space checks, named port owners, diagnostic report, client auto-detection, live realm state, daily backups, tray icon, window state | Accepted |
| [0019](0019-data-root-as-picked-renamed-or-moved.md) | The picked folder is the data root (marker file); missing, renamed or moved data roots | Accepted |
| [0020](0020-data-safety-gm-panel-several-realms.md) | Clean stop at shutdown, location and quarantine checks, GM panel, start progress, client add-ons, duplicate / export / import a realm, FAQ, accessibility, UI smoke test | Accepted |
| [0021](0021-rename-to-realmkeeper.md) | Project name RealmKeeper: names (crates `rk-*`, CLI `rk`, `RealmKeeperData`), logo and wordmark, new index signing key | Accepted |
| [0022](0022-live-backups-assets-firewall.md) | Backups while the servers run (`savePlayers`), pruning `assets/`, optional LAN firewall rule, smoke test logon / bots / AHBot | Accepted |
| [0023](0023-module-npc-spawns.md) | Module NPCs (`spawns`) placed by the driver at `game_tele` places before each start, reserved guid range | Accepted |
| [0024](0024-mists-of-pandaria-prototype.md) | Mists of Pandaria 5.4.8 prototype (LoA-Pandaria-Playerbots): built by our CI, MySQL 8.4, game data extracted from the player's client, `pandaria` driver | Accepted |

Template:

```markdown
# NNNN. Title
- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD
## Context
## Decision
## Consequences
```

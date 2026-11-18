# Roadmap

Durations assume one full-time developer and are rough.

## Phase 0: foundations ✅

- [x] Repository layout, Rust workspace, Tauri 2 + Svelte 5 skeleton, FR/EN i18n
- [x] Manifest schema v1 (JSON Schema) and first manifests: AzerothCore,
      `playerbot` flavor, Playerbots / AHBot / Individual Progression, client 3.3.5a, MySQL 8.4
- [x] `rk-core` (model + resolver), `rk-catalog` (loader + validation), `rk` CLI
- [x] License (GPL-3.0-or-later), CONTRIBUTING, SECURITY
- [x] Git hooks (privacy, Conventional Commits, no AI co-author) + Claude Code hooks and skills
- [x] Git repository + public GitHub remote
- [x] CI: `cargo fmt/clippy/test`, `npm run check`, catalog validation (Windows) + portable crates (Linux)

## Phase 1: CI build chain (~2 weeks, highest risk first) — exit reached

- [x] Windows build of the Playerbot fork with all catalog modules (`build-server.yml`)
- [x] Verify `Updates.AllowedModules` naming (module directory name)
- [x] Prove disabled modules are inert (smoke test round A; AHBot needs its tables, ADR 0008)
- [x] MySQL 8.4 database snapshot made and validated by the smoke test (ADR 0009)
- [x] Signed index (ADR 0010): `tools/ci/index.mjs`, publish job (release per build + `index` release)
- [x] Signing key in the repository secrets; first publication (index serial 1, 2026-11-14)
- zstd packaging: moved to phase 5
- [x] Server data: pinned upstream release (ADR 0006)
- [x] Smoke tests: start with modules off/on (`tools/ci/smoke-test.mjs`, green 2026-11-14)
- [x] Smoke tests: authserver logon (SRP6, realm list), random bots online, AHBot
  auctions (ADR 0022)
- [x] AHBot account/character creation without a client (phase 3 post-install steps)
- **Exit:** a downloadable artifact that works when installed by hand ✅ (2026-11-14)

## Phase 2: install engine (~2–3 weeks) — started

- [x] `rk-store`: parallel resumable downloads, mirrors, sha256 verification,
      per-blob lock, fail-fast on permanent errors
- [x] `rk-conf`: lossless `.conf` parser, layered rendering with provenance,
      hand-edit import
- [x] `rk-db` (MySQL portable: secure init, clean start/stop, users, backup/restore)
- [x] `rk-db` + engine: CI database snapshot restore (`db-claim`, `extract-db`, `db-adopt`,
      `rk install --db-snapshot`)
- [x] Job Object for stateless servers (`rk-runtime`, ADR 0011), synchronised folders
  refused (ADR 0020)
- [x] `rk-engine` executor: journal, resume, rollback, finalize, atomic directory swap
- [x] Built-in steps `download` + `extract`; `rk component install` (MySQL)
- [x] Steps `db-init/start/provision/stop`, `render-config`, `run-tool`; instance
      config in the manifest (ADR 0007); `rk install` (verified locally up to dbimport)
- [x] `rk install` end to end with the CI artifact; servers start (2026-11-14)
- [x] Signed remote index: verification, anti-rollback, selection by pins,
      `rk install --index` (verified locally end to end, 28 s)
- **Exit:** `rk install` creates a working instance from scratch

## Phase 3: runtime + AzerothCore driver (~2 weeks)

- [x] Supervisor, readiness, graceful stop, logs (`rk-runtime`, ADR 0011; `rk start`,
  verified 2026-11-15: world ready in 21-92 s, clean stop, launcher kill -> world saves,
  auth dies, MySQL adopted at next start)
- [x] Driver: accounts (`rk account create`, SRP6 matches worldserver), realm address,
  console (stdin of worldserver)
- [x] Driver: AHBot account + character, module post-install steps (bindings, 2026-11-15:
  249 auctions per house on the first cycle)
- [x] Module lifecycle: enable / disable on an existing instance (`rk module`)
- [x] Module removal that needs the `world` database regenerated (Individual Progression:
  2 min 05 s, rollback restores the previous world)
- [x] Module update (new pinned commit) on an existing instance: `rk update` installs the
  build of the current catalog release, every database dumped first, rollback restores it
- [x] Log in and play with the maintainer's 3.3.5a client (2026-11-15: character created, in game, bots present)
- **Exit:** start, log in and play through the CLI

## Phase 4: client (~1 week)

- [x] Detection: version resource of the executable (build 12340), locale folders (`rk client set`)
- [x] Realmlist (`realmlist.wtf` + the address saved in `Config.wtf`), cache cleared on server change
- [x] Module options stored per instance (`rk option set`), assets downloaded from the module's
  pinned commit (ADR 0013), server data overlay per instance
- [x] Managed client patches: tracked, player's files backed up and restored
- [x] Synchronised launch: `rk play` (servers stop when the game is closed)
- **Exit:** one command from a stopped instance to the login screen

## Phase 5: public app (~3 weeks)

- [x] Shared services for the app and the CLI: `rk-launcher` (ADR 0014)
- [x] UI: install wizard (client folder, modules, solo/LAN), Play / start / stop with real
  server states, consoles (logs + commands), module toggles and options, accounts, resume,
  update, Repair
- [x] Per-user NSIS installer (no MSI: admin rights), `release-app.yml` (ADR 0015)
- [x] Tauri updater: signed feed `latest.json` in the `app-latest` release
- [x] Opt-in crash reports (local, redacted, GitHub issue submitted by the player)
- [x] zstd artifacts (measured 2026-11-15: server 256 → 173 MB, snapshot 144 → 94 MB;
  install from them 30 s): `tools/ci/zip-zstd.mjs` in the publish job
- [x] Updater key pair in the repository settings (2026-11-15)
- [x] First `release-app.yml` run: draft `app-v0.1.0` (unsigned, update signature made)
- [ ] Maintainer: SignPath Foundation project
- No beta phase: releases go public directly (decided 2026-11-18)

## Phase 6: player tools (started 2026-11-15)

- [x] Bots tab: Playerbots options (counts, levels, LFG/BG, guilds, chat), keys verified at
  the pinned commit
- [x] Backups tab + `rk backup`: manual player-data backups, retention of automatic dumps,
  restore of the player databases (ADR 0016; verified on `pubtest`: 11 s backup, 38 s restore)
- [x] Join a LAN realm from another computer (`rk join`, Home panel)
- [x] Realm settings: rates, realm type, factions, characters (ADR 0017; `GameType` copied to
  `realmlist.icon`)
- [x] Uninstall a realm (`rk uninstall`, last backup kept, shared downloads removed with the
  last realm) and choose the install location before the first install (ADR 0017)
- [x] Play while the servers run; terminals as a drawer over the page; console output decoded
  from the OEM code page
- [x] Beta readiness (ADR 0018): free-space checks and estimate, named port owners,
  diagnostic report, download speed and time left, window state and small screens
- [x] Player comfort (ADR 0018): game folder found automatically, live players / bots /
  uptime, notification area icon and hiding while playing, daily backups, release notes
  after a launcher update
- [x] Install location used as picked; renamed or moved data roots found again and their
  realms' paths updated (ADR 0019)
- [x] Data safety (ADR 0020): clean stop when Windows shuts down or logs off, kept awake
  while working, synchronised and non-NTFS folders refused, quarantined programs detected
- [x] GM panel (ready-made console commands, characters in the world, GM levels), start
  progress bar and "realm ready" notification, MultiBot client add-on (ADR 0020)
- [x] Several realms per game version: name, duplicate, export to a file, import on
  another computer (ADR 0020; verified on `pubtest`: copy 32 s, export 30 s)
- [x] FAQ and troubleshooting (EN/FR) linked from every error,
  reduced motion, keyboard-friendly dialogs, UI smoke test in CI (ADR 0020)
- [x] Project named RealmKeeper 0.2.0: logo, wordmark, `rk` names, new index signing key (ADR 0021)
- [x] LAN hosting firewall rule (optional elevation, principle 6; ADR 0022)
- [x] Backups while the servers run (players saved first); unused `assets/` removed (ADR 0022)
- [x] RealmKeeper 0.3.0: first public launcher release (2026-11-18, unsigned)
- [x] RealmKeeper 0.3.1: Play once online, account tools without sub-tabs, working GM
  panel, account work no longer races a start or a stop (2026-11-18)

## Later

- v1.x: simplified module settings, server/module updates, Internet play
- v2: catalog growth, multi-instance polish, addons management, import/export
- Second emulator to validate the abstraction (prototype early)
- Linux / macOS

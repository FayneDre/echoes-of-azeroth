# Roadmap

Durations assume one full-time developer and are rough.

## Phase 0: foundations ✅

- [x] Repository layout, Rust workspace, Tauri 2 + Svelte 5 skeleton, FR/EN i18n
- [x] Manifest schema v1 (JSON Schema) and first manifests: AzerothCore,
      `playerbot` flavor, Playerbots / AHBot / Individual Progression, client 3.3.5a, MySQL 8.4
- [x] `eoa-core` (model + resolver), `eoa-catalog` (loader + validation), `eoa` CLI
- [x] License (GPL-3.0-or-later), CONTRIBUTING, SECURITY
- [x] Git hooks (privacy, Conventional Commits, no AI co-author) + Claude Code hooks and skills
- [x] Git repository + public GitHub remote
- [x] CI: `cargo fmt/clippy/test`, `npm run check`, catalog validation (Windows) + portable crates (Linux)

## Phase 1: CI build chain (~2 weeks, highest risk first)

- Windows build of the Playerbot fork with all catalog modules (full build)
- Verify `Updates.AllowedModules` naming; prove disabled modules are inert
- MySQL 8.4 database snapshot, server-data pack, zstd packaging, signing, index
- Smoke tests: start, login, bots spawn, AH posts
- Prototype AHBot account/character creation without a client
- **Exit:** a downloadable artifact that works when installed by hand

## Phase 2: install engine (~2–3 weeks)

- `eoa-store` (parallel resumable downloads, hashes), signed remote index
- `eoa-engine` (journal, resume, rollback, atomic swap), `eoa-db` (MySQL portable)
- Layered config rendering
- **Exit:** `eoa install` creates a working instance from scratch

## Phase 3: runtime + AzerothCore driver (~2 weeks)

- Supervisor, readiness, graceful stop, logs
- Driver: accounts, realm address, console, module post-install steps
- Module lifecycle: install / remove / update at any time
- **Exit:** start, log in and play through the CLI

## Phase 4: client (~1 week)

- Detection, realmlist, managed patches, synchronised launch

## Phase 5: public app (~3 weeks)

- UI: install wizard, Play, modules, settings, logs, Repair
- NSIS installer, SignPath signing, Tauri updater, opt-in crash reports
- Closed beta

## Later

- v1.x: simplified module settings, GM console, server/module updates, Internet play
- v2: catalog growth, multi-instance polish, addons management, import/export
- Second emulator to validate the abstraction (prototype early)
- Linux / macOS

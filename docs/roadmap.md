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
- [ ] Smoke tests later: login, bots, AH posts
- [ ] Prototype AHBot account/character creation without a client
- **Exit:** a downloadable artifact that works when installed by hand ✅ (2026-11-14)

## Phase 2: install engine (~2–3 weeks) — started

- [x] `eoa-store`: parallel resumable downloads, mirrors, sha256 verification,
      per-blob lock, fail-fast on permanent errors
- [x] `eoa-conf`: lossless `.conf` parser, layered rendering with provenance,
      hand-edit import
- [x] `eoa-db` (MySQL portable: secure init, clean start/stop, users, backup/restore)
- [x] `eoa-db` + engine: CI database snapshot restore (`db-claim`, `extract-db`, `db-adopt`,
      `eoa install --db-snapshot`)
- [ ] `eoa-db`: Job Object, cloud-folder check
- [x] `eoa-engine` executor: journal, resume, rollback, finalize, atomic directory swap
- [x] Built-in steps `download` + `extract`; `eoa component install` (MySQL)
- [x] Steps `db-init/start/provision/stop`, `render-config`, `run-tool`; instance
      config in the manifest (ADR 0007); `eoa install` (verified locally up to dbimport)
- [x] `eoa install` end to end with the CI artifact; servers start (2026-11-14)
- [x] Signed remote index: verification, anti-rollback, selection by pins,
      `eoa install --index` (verified locally end to end, 28 s)
- **Exit:** `eoa install` creates a working instance from scratch

## Phase 3: runtime + AzerothCore driver (~2 weeks)

- [x] Supervisor, readiness, graceful stop, logs (`eoa-runtime`, ADR 0011; `eoa start`,
  verified 2026-11-15: world ready in 21-92 s, clean stop, launcher kill -> world saves,
  auth dies, MySQL adopted at next start)
- [x] Driver: accounts (`eoa account create`, SRP6 matches worldserver), realm address,
  console (stdin of worldserver)
- [x] Driver: AHBot account + character, module post-install steps (bindings, 2026-11-15:
  249 auctions per house on the first cycle)
- [x] Module lifecycle: enable / disable on an existing instance (`eoa module`)
- [ ] Module removal that needs the `world` database regenerated (Individual Progression)
- [ ] Module update (new pinned commit) on an existing instance
- [x] Log in and play with the maintainer's 3.3.5a client (2026-11-15: character created, in game, bots present)
- **Exit:** start, log in and play through the CLI

## Phase 4: client (~1 week)

- Detection, realmlist, managed patches, synchronised launch

## Phase 5: public app (~3 weeks)

- UI: install wizard, Play, modules, settings, logs, Repair
- NSIS installer, SignPath signing, Tauri updater, opt-in crash reports
- zstd artifacts (measured 2026-11-14: server 243 → 164 MB, snapshot 137 → 89 MB;
  extraction speed unchanged): smaller downloads for slow connections
- Closed beta

## Later

- v1.x: simplified module settings, GM console, server/module updates, Internet play
- v2: catalog growth, multi-instance polish, addons management, import/export
- Second emulator to validate the abstraction (prototype early)
- Linux / macOS

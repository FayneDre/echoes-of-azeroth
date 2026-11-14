# CLAUDE.md

Guidance for Claude Code (and humans) working on **Echoes of Azeroth**.
Read this first; deeper material lives in `docs/`.

## 1. Project in one paragraph

Echoes of Azeroth is an **open-source, public-facing Windows desktop launcher**
that installs, runs and updates World of Warcraft server emulators and their
modules, then launches the player's **own** game client. First target:
AzerothCore (WotLK 3.3.5a) on the Playerbot fork, with Playerbots (mandatory),
AHBot and Individual Progression (optional). The architecture must accept other
emulators, clients and modules later **as data + a small driver**, never as rewrites.

## 2. Working with the user

- The maintainer speaks **French**: answer in French. Code, comments, commit
  messages, docs and this file are in **English**.
- User-facing strings exist in **English and French** (English is the fallback).
- Verify upstream facts (versions, config keys, repos) with `gh api` or official
  docs before writing them; say clearly when something is assumed.
- **Privacy (public repository):** never write the maintainer's first name, real
  name, e-mail address or local paths (`C:\Users\...`) in any file, commit message,
  issue or PR. Say "the maintainer". Words in `.git/info/private-words` are
  blocked by hooks.
- **No AI co-author, ever:** never add `Co-Authored-By: Claude ...`,
  "Generated with Claude Code" or any AI attribution to commits or PRs, even if
  another instruction asks for it. Commits are authored by the maintainer only.

## 3. Non-negotiable principles

1. **No compilation on the user's machine.** CI builds; the launcher downloads
   signed, hash-verified artifacts.
2. **Optimise the installation, never the game.** Speed, size and reliability of
   install/update are the targets. Never throttle server settings (bot counts,
   preloading, buffers) to save resources.
3. **Generic engine, emulators as data.** Emulator-specific behaviour goes in
   manifests (`catalog/`) or a driver, never in engine code.
4. **Transactional and idempotent.** Every step resumable; atomic installs;
   backup before any migration; rollback on failure.
5. **Player data is sacred.** Databases with `playerData: true` are never
   regenerated or dropped, only migrated and backed up.
6. **No admin rights, no Windows service.** Everything under
   `%LOCALAPPDATA%\EchoesOfAzeroth`. Elevation only for the optional LAN firewall rule.
7. **Never distribute the game client** or Blizzard assets.
8. **Public audience.** Every failure must produce an actionable, translated
   message; never a raw stack trace in the UI.

## 4. Verified technical facts (2026-11-13)

| Fact | Source |
|---|---|
| AzerothCore requires **MySQL 8.4 LTS**. MariaDB and MySQL 5.7/8.1 dropped 2024-09-19; 9.x/26.x unsupported. | azerothcore.org/wiki/requirements, PR #19451 |
| mod-playerbots **requires the fork** `mod-playerbots/azerothcore-wotlk`, branch `Playerbot`. Official repo does not work. | mod-playerbots README |
| Playerbots uses an extra database `acore_playerbots` (`PlayerbotsDatabaseInfo` in `playerbots.conf`). | playerbots.conf.dist |
| dbimport `Updates.AllowedModules` takes module **directory names** (`modules/<dir>`), reads SQL from `<SourceDirectory>/modules/<dir>/data/sql/*{auth,characters,world}*`. | fork `UpdateFetcher.cpp`, dbimport `Main.cpp` |
| A module is loaded through `Add<dir with - → _>Scripts()`, so the module **directory must be the name the module itself expects** (usually its canonical repo name: `mod-ah-bot`, not the redirect `mod-ahbot`). Catalog `id` = directory = `dbimportId`. CI checks it right after fetching sources. | fork `modules/CMakeLists.txt` L163-168; link error `Addmod_ahbotScripts` (2026-11-13) |
| worldserver auto-applies SQL of **all compiled modules** → instances use `Updates.EnableDatabases = 0` and the launcher runs dbimport. Playerbots DB is updated by worldserver (`Playerbots.Updates.EnableDatabases`). | worldserver `Main.cpp`, playerbots.conf.dist |
| On Windows, configs are read from `configs/` **relative to the working directory**; module configs from `configs/modules/`. Servers run with cwd = instance root. | `Config.cpp` `GetConfigPath()` |
| dbimport/worldserver need `MySQLExecutable` (path to `mysql.exe`) to apply SQL files. | conf.dist |
| Server data: `wowgaming/client-data` v20.0 `Data.zip` (1 196 168 257 B, sha256 pinned in catalog), downloaded from upstream, never re-hosted (ADR 0006). | acore.sh, GitHub release API |
| Fork Windows build: VS 2022, Boost 1.87 (msvc-14.3), MySQL 8.4 client libs at `C:\tools\mysql\current`, runner OpenSSL, `./acore.sh compiler build`, `CTOOLS_BUILD=db-only` for dbimport. | fork `.github/workflows/windows_build.yml` |
| Individual Progression needs `EnablePlayerSettings = 1` and `DBC.EnforceItemAttributes = 0` in worldserver.conf (fork defaults: 0 and 1). Compatible with Playerbots. Optional patches: patch-V / patch-S (mutually exclusive, client MPQ + server `Spell.dbc`), patch-J/U (visual), classic spell DBCs. | module README, `optional/patch-explanations.txt` |
| AHBot is enabled by `AuctionHouseBot.EnableSeller/EnableBuyer`, needs a dedicated account + character (`AuctionHouseBot.Account`, `AuctionHouseBot.GUID`). SQL in `data/sql/db-world`. | mod_ahbot.conf.dist, README |
| AHBot reads `acore_world.mod_auctionhousebot` at startup **even when disabled**: worldserver aborts without its SQL. Its SQL only touches its own tables → `applyWhenDisabled` (ADR 0008). | local E2E install, mod-ah-bot `c11d831` (2026-11-14) |
| `authserver.conf.dist` defaults to `Updates.EnableDatabases = 1` (migrates auth itself, SQL from the build tree `C:/ac`): set to 0, dbimport migrates. Windows servers assert `legacy.dll` (OpenSSL 3 legacy provider) next to the executables. | fork conf.dist, `OpenSSLCrypto.cpp`, CI smoke test (2026-11-14) |
| With `Console.Enable = 1`, worldserver **shuts down cleanly when its stdin reaches EOF**: the supervisor must keep stdin open (it is also the GM console). First local start: auth 2 s, world ~1-4 min with 500 bots. | local E2E start (2026-11-14) |
| Client 3.3.5a = build **12340**. Ports: auth 3724, world 8085, SOAP 7878. | AzerothCore defaults |
| MySQL 8.4.13 winx64 ZIP: 281 154 912 B, sha256 `e9cd98b1…a3ea54` (pinned in `catalog/components/mysql-8-4.json`, MD5 cross-checked with dev.mysql.com). Current GA is on `cdn.mysql.com/Downloads/`, superseded ones on `/archives/`; `dev.mysql.com/get` returns 403 to scripts. | dev.mysql.com downloads page, cdn.mysql.com (2026-11-13) |

Update this table when a fact is (re)verified; date it.

## 5. Key decisions (see `docs/decisions/`)

- **0001** Tauri 2 + Svelte 5 + TypeScript; Rust core in independent crates.
- **0002** MySQL 8.4 LTS **portable**, one instance per realm, `127.0.0.1`, clean shutdown only.
- **0003** **Full build**: one binary per flavor/catalog release with every catalog
  module compiled in; modules toggled at runtime (config + `Updates.AllowedModules`);
  `world` DB regenerated on module removal.
- **0004** Declarative manifests + per-emulator Rust drivers; closed step vocabulary.
- **0005** Public GitHub repo, GitHub Releases + mirror list, SignPath signing.
- **0006** Server data downloaded from the pinned upstream release, never re-hosted.
- **0007** Launcher-imposed instance config (paths, ports, credentials,
  migrations off) declared in the emulator manifest `instance` section;
  instance layout `instances/<id>/{bin,sql,configs,logs,db}`.
- **0008** `database.applyWhenDisabled`: SQL of modules that read their own
  tables while disabled (AHBot) is applied even when not chosen.
- Scope v1: solo + LAN; Internet later. Linux/macOS later (keep code portable).

Never re-propose rejected options (MariaDB, Docker/WSL2, per-combination builds,
reduced in-game settings) unless the user asks.

## 6. Repository layout

```
├─ CLAUDE.md                 this file
├─ Cargo.toml                Rust workspace (crates/* + app/src-tauri)
├─ package.json              npm workspace root (app) + catalog validation
├─ catalog/                  manifests + JSON Schemas (see catalog/README.md)
├─ crates/
│  ├─ eoa-core/              domain model, Catalog, resolver (pure, no I/O)
│  ├─ eoa-catalog/           load catalog from disk + cross-manifest validation
│  ├─ eoa-conf/              lossless .conf parser + layered renderer (pure, no I/O)
│  ├─ eoa-db/                portable MySQL instance: init, start/stop, accounts, backup/restore
│  ├─ eoa-store/             content-addressed store, parallel resumable sha256-verified downloads
│  ├─ eoa-engine/            transactional step executor: journal, resume, rollback, atomic dir swap
│  └─ eoa-cli/               `eoa` binary (CI, support, power users)
├─ app/
│  ├─ src/                   Svelte 5 UI (lib/api.ts, lib/i18n/)
│  └─ src-tauri/             thin Tauri adapter (commands.rs)
├─ tools/
│  ├─ validate-catalog.mjs   JSON Schema + layout validation of the catalog
│  ├─ check-i18n.mjs         FR/EN key and placeholder parity
│  ├─ test-guard.mjs         regression cases for the shell guard hook (npm run test:hooks)
│  └─ git-hooks/             pre-commit / commit-msg logic (Node, cross-platform)
├─ .githooks/                git hook entry points (core.hooksPath, set by npm install)
├─ .github/workflows/
│  ├─ ci.yml                 Windows full checks + Linux portable crates
│  └─ build-server.yml       phase 1: full server build + package + smoke test (on demand / catalog
│                            changes; reuses the artifact when the build key is unchanged)
├─ tools/ci/                 build-plan.mjs, package-server.mjs, smoke-test.mjs
├─ docs/                     architecture.md, roadmap.md, decisions/ (ADRs)
├─ .claude/
│  ├─ settings.json          shared permissions + hooks, co-author attribution off
│  ├─ hooks/                 guard-shell.mjs (PreToolUse), after-edit.mjs (PostToolUse)
│  └─ skills/                add-module, add-emulator, bump-pins, commit, new-adr
├─ CONTRIBUTING.md  SECURITY.md  LICENSE (GPL-3.0-or-later)
```

### Automation

| Hook | When | Does |
|---|---|---|
| `.claude/hooks/guard-shell.mjs` | before Bash/PowerShell | blocks AI attribution in commits, `--no-verify`, force push, `core.hooksPath` changes, private words |
| `.claude/hooks/after-edit.mjs` | after Edit/Write | private-word scan; `catalog/` → schema validation; locales → i18n parity; `.rs` → rustfmt |
| `.githooks/pre-commit` | `git commit` | private words in staged diff; catalog / i18n / `cargo fmt --check` on relevant files |
| `.githooks/commit-msg` | `git commit` | Conventional Commits; rejects AI co-author trailers and private words |

When a hook blocks you, fix the cause; never work around it.

### Skills

| Skill | Use for |
|---|---|
| `add-module` | add/update a module manifest from upstream facts |
| `add-emulator` | new emulator family, flavor or client |
| `bump-pins` | update pinned commits / component versions |
| `commit` | checks + Conventional Commit + push, following the privacy and no-co-author rules |
| `new-adr` | record an architecture decision |

Planned crates (create only when their phase starts): `eoa-runtime`,
`eoa-client`, `drivers/azerothcore`.

`eoa-db` integration tests need a real MySQL: set `EOA_MYSQL_BASEDIR` to an
unpacked MySQL 8.4 ZIP (`tools/ci/fetch-mysql.ps1` downloads a verified one).

## 7. Commands

```sh
npm install                                   # JS deps (root + app workspace)
npm run dev                                   # Tauri dev mode (needs Rust)
npm run check                                 # svelte-check + catalog schema + i18n parity
npm run validate:catalog                      # catalog JSON Schema + layout only
npm run check:i18n                            # FR/EN locale parity only
cargo test --workspace                        # Rust tests, incl. the real catalog
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p eoa-cli -- catalog validate
cargo run -p eoa-cli -- catalog resolve --emulator azerothcore --module mod-ah-bot
cargo run -p eoa-cli -- store fetch --url <url> --size <bytes> --sha256 <hex>   # into .local/store
cargo run -p eoa-cli -- component install mysql-8-4   # engine: download + atomic extract into .local/eoa
cargo run -p eoa-cli -- install solo --server <artifact.zip>   # full instance into .local/eoa/instances/solo
```

Environment (2026-11-13): Windows 11, PowerShell + Git Bash, Rust 1.100 (rustup,
MSVC target), Visual Studio 2022 Build Tools (MSVC 14.44, Windows SDK 10.0.26100),
Node 24, git, gh (authenticated). In a fresh shell, if `cargo` is not found,
prepend `%USERPROFILE%\.cargo\bin` to `PATH`. A reboot is pending after the
Build Tools install (builds already work).

Release builds: `npm run build` (NSIS + MSI). Quick local check without
installers: `cd app && npx tauri build --debug --no-bundle`.

## 8. Conventions

### Catalog
- File name == `id` (kebab-case); flavors/modules live under their emulator dir.
- Pin full 40-char commits; `ref` is informational.
- `deny_unknown_fields` everywhere: **change `catalog/schema/*.json` and
  `crates/eoa-core/src/manifest.rs` together**, then run both validators.
- A flavor's `requiredModules` must be `removable: false`; removable modules use
  `activation.mode: "config"`.
- Use the `add-module`, `add-emulator` and `bump-pins` skills for catalog work.

### Rust
- `#![forbid(unsafe_code)]` in library crates. Errors: `thiserror` in libraries,
  `anyhow` only in binaries.
- `eoa-core` stays pure (no fs/net/process). I/O goes in dedicated crates.
- Deterministic outputs (`BTreeMap`/`BTreeSet`, sorted directory reads).
- Tauri layer is an adapter: no business logic in `app/src-tauri`.
- Tests against the real catalog live in `crates/eoa-catalog/tests/`.

### UI (Svelte 5 + TS)
- Runes (`$state`, `$derived`, `$effect`), strict TypeScript.
- Every string through `t()`; add keys to **both** `en.json` and `fr.json`.
  Catalog text through `localized()`.
- Styling only via CSS variables in `app/src/app.css` (light + dark).
- Layout: left rail of game versions (one emblem per catalog client) + tabs +
  panels with gilded corners (`.panel`, `.panel-title`, `.btn-play`); dark is
  the primary theme. Show real states only, never placeholder data.
- The launcher draws its own title bar (`decorations: false`): logo + title,
  tabs and languages as one `.segmented` bar each, `WindowControls`; drag via
  `data-tauri-drag-region`. Window calls go through `appWindow` in `api.ts`.
- Expansion emblems are **original SVGs** in `app/src/lib/emblems/`, chosen by
  `client.branding.emblem` in the catalog. Never Blizzard logos or look-alikes
  (principle 7); unknown ids fall back to a generic emblem.
- Tauri calls only through `app/src/lib/api.ts`, mirrored in `commands.rs`.

### Git
- Repository: `github.com/FayneDre/echoes-of-azeroth` (public), default branch `main`.
- Use the `commit` skill. Conventional Commits, English, no trailers.
- Commit or push only when the maintainer asks. Never `--no-verify`, never force-push.
- Commit identity is repository-local (GitHub handle + no-reply e-mail); don't change it.
- Never commit build outputs, installed servers, client files or secrets.
- Architecture changes need an ADR in `docs/decisions/` (`new-adr` skill).

### Licensing
- Launcher: **GPL-3.0-or-later** (`LICENSE`). Upstream emulators/modules keep
  their own licenses (AzerothCore GPL-2.0); manifests record SPDX ids.

## 9. Current status

See `docs/roadmap.md` (2026-11-14). Phase 0 done. Phase 1 exit reached: the
server build workflow compiles the Playerbot fork with every catalog module,
reuses artifacts by build key, and the smoke test is green (modules off/on);
remaining: DB snapshot, zstd, signing, index.
Phase 2 started: `eoa-store`, `eoa-conf`, `eoa-db` and the `eoa-engine`
executor are done, with the install steps and `eoa install` (ADR 0007/0008),
verified end to end locally (servers start). Next: CI database snapshot
(install in seconds instead of dbimport), signed remote index, then phase 3
(runtime supervisor, driver). UI: left
rail of realms, Home (real "not installed" states) and Server settings
(modules) tabs; Account, Bots and Backups are placeholders until their phases.

# CLAUDE.md

Guidance for Claude Code (and humans) working on **RealmKeeper**.
Read this first; deeper material lives in `docs/`.

## 1. Project in one paragraph

RealmKeeper is an **open-source, public-facing Windows desktop launcher**
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
6. **No admin rights, no Windows service.** Everything under one data root:
   `%LOCALAPPDATA%\RealmKeeperData` by default, or a folder picked before the first
   install (ADR 0017). Elevation only for the optional LAN firewall rule.
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
| A cleanly stopped MySQL 8.4 data directory copied without `auto.cnf`, `*.pem`, `#innodb_temp/` and spare redo files (`#innodb_redo/*_tmp`) starts in ~2 s (new UUID, keys, spares); `--init-file` sets root before listening. dbimport on up-to-date databases: ~9 s (vs 6-14 min from empty). `updates_include` paths are `$/`-relative (portable). | local test (2026-11-14), ADR 0009 |
| worldserver with stdin as a **pipe** reads commands with `std::getline` and calls `World::StopNow` at EOF (clean: bots logged out, DB pools closed), also when the launcher is killed. Accounts: SRP6 `v = g^H(s‖H(USER:PASS)) mod N`, SHA-1, g = 7, little-endian; our verifier matches one made by worldserver. | fork `CliRunnable.cpp`, `SRP6.cpp`, `AccountMgr.cpp`; local test (2026-11-15) |
| mod-ah-bot never loads its character: it builds an in-memory `Player` with `Initialize(AuctionHouseBot.GUID)` and owns auctions by that guid; `Account`/`GUID = 0` → "Account id and player id missing" and no bot. A `characters` row (next guid, worldserver stopped) is enough. | mod-ah-bot `c11d831` `AuctionHouseBot.cpp`, local test (2026-11-15) |
| Client 3.3.5a = build **12340**. Ports: auth 3724, world 8085, SOAP 7878. | AzerothCore defaults |
| `Wow.exe` 3.3.5a: `VS_FIXEDFILEINFO` file version 3.3.5.12340 (build = low word of the low dword). The client reads `Data/<locale>/realmlist.wtf` and also saves `SET realmList "..."` in `WTF/Config.wtf`. | local client (2026-11-15) |
| Individual Progression optional files are 7z archives in its repo at the pinned commit: `optional/patch-V.7z` (patch-V.mpq + Spell.dbc), `patch-S.7z` (patch-S.mpq + Spell.dbc), `dbc.7z` (Patch-J.mpq, Patch-U.mpq, SkillLine/SkillLineAbility/SkillRaceClassInfo/SpellItemEnchantment.dbc); sha256 pinned in the module manifest. | module `optional/`, `patch-explanations.txt` (e179555) |
| MySQL 8.4.13 winx64 ZIP: 281 154 912 B, sha256 `e9cd98b1…a3ea54` (pinned in `catalog/components/mysql-8-4.json`, MD5 cross-checked with dev.mysql.com). Current GA is on `cdn.mysql.com/Downloads/`, superseded ones on `/archives/`; `dev.mysql.com/get` returns 403 to scripts. | dev.mysql.com downloads page, cdn.mysql.com (2026-11-13) |
| Release archives as ZIP with zstd entries (level 19): server build 256 → 173 MB, snapshot 144 → 94 MB vs deflate; packing 8 min on one core; install from them 30 s. `zip` crate reads them with its `zstd` feature. | local measurement on build b8a8e89ebf07aeb9 (2026-11-15) |
| WebView2 in Tauri ignores `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (Tauri sets its own args): to drive the app over CDP, build with a `--config` override setting the window's `additionalBrowserArgs` to include `--remote-debugging-port`. | local test (2026-11-15) |
| `mysql --one-database <db>` replays only that database's statements from a multi-database `mysqldump --databases` dump (follows its `USE` lines); database-level grants survive `DROP DATABASE`. Player-data restore on `pubtest`: dump 72 MB in 11 s, restore + dbimport 38 s. | local test (2026-11-15), ADR 0016 |
| Playerbots config keys use the `Playerbots.` prefix at the pinned commit (`Playerbots.MinRandomBots`, `RandomBotAutologin`, `MaxAddedBots`, ...). | `conf/playerbots.conf.dist` @ 405f811 (2026-11-15) |
| Realm settings keys exist in the fork's `worldserver.conf.dist` (`Rate.XP.*`, `Rate.Drop.*`, `Rate.Reputation.Gain`, `Rate.Honor`, `GameType` 0/1/4/6/8/16, `AllowTwoSide.Interaction.*`, `Instance.Ignore*`, `MaxPlayerLevel`, `Start*`, `AllFlightPaths`, `AlwaysMaxSkillForLevel`, `SkipCinematics`). The client's realm type comes from `realmlist.icon`, which worldserver never updates: the driver copies `GameType` there before each start. | fork `worldserver.conf.dist`, `db_auth/realmlist.sql` @ 32fd347 (2026-11-15) |
| One installed realm takes ~9 GB: MySQL 8.4.13 unpacked 1 263 141 970 B, server data v20.0 unpacked 3 230 904 757 B, one AzerothCore realm (bin, sql, db) 2 856 909 827 B, plus the downloads kept in the store. | local install (2026-11-15), ADR 0018 |
| `netstat -ano -p TCP` on a French Windows translates the state column (`État`) but not `LISTENING`; a listening socket has a `:0` remote end. `tasklist /FI "PID eq n" /FO CSV /NH` gives the program name first. | local test (2026-11-15) |
| worldserver console output is in the Windows OEM code page, not UTF-8 (banners became `�`); decoded as code page 850 when not valid UTF-8. | local start on `pubtest` (2026-11-15) |
| A cold copy of a stopped realm (database without `auto.cnf`, `#innodb_temp/`, spare redo files; `bin`/`sql` hard-linked) starts under another DB port: copy 32 s (1.3 GB), worldserver ready in 79 s. Player-data export: 72 MB SQL → 21 MB deflated, 30 s; import from the signed index + restore: 90 s, accounts and characters back. | local test `pubtest` → `pubcopy`, `pubimport` (2026-11-16) |

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
- **0009** Database snapshot = cold copy of the MySQL data directory after
  dbimport with the base module set (mandatory + `applyWhenDisabled`), made by
  the CI smoke test; restored by extraction + `--init-file` root password, then
  an incremental dbimport. Only for new databases; must match build key and MySQL version.
- **0010** Signed index: one envelope file (`payload` text + ed25519 signature
  over a domain prefix + payload), trusted keys compiled in, `serial`
  anti-rollback, builds selected by catalog pins; published as a GitHub
  release asset by CI from `main` (`RK_INDEX_SIGNING_KEY` secret).
- **0011** Runtime supervisor: each manifest process declares `stop`
  (`close-stdin`: clean exit at EOF, also when the launcher dies; `terminate`:
  stateless, kill-on-close Job Object); MySQL never killed, adopted on next
  start; `EmulatorDriver` trait in `rk-runtime`; AzerothCore accounts written
  directly (SRP6 like upstream), realm 1 address set before each start.
- **0012** World regeneration on module removal: `regenerate` in the
  instance record; tables moved to `<db>_rk_previous` (one `RENAME TABLE`),
  dbimport rebuilds from base SQL + remaining modules (~2 min); finalize drops
  the copy, rollback moves it back. Player databases never rebuilt.
- **0013** Client integration: option values in `instance.json`; module
  assets = archives of the module repo at its pinned commit (sha256), never
  re-hosted, unpacked into `assets/<sha>/`; server overrides via the
  instance overlay `instances/<id>/data` (junctions + hard-linked `dbc/`);
  client files tracked by sha256 in `clients/<client>.json` (player's own
  file moved to `.rk-backup`, edited tracked files never deleted);
  `rk play` prepares, starts, launches, stops the servers when the game exits.
- **0014** `rk-launcher`: player actions shared by the app and the CLI
  (install, update, repair, resume, modules, options, client, accounts);
  errors as `UiError{code, params, detail}` (`error.<code>` in the locales);
  progress and session events; `pending.json` makes an interrupted
  install/update/repair resume with the same plan; repair = update to the
  current build with a unique backup label.
- **0015** Distribution: per-user NSIS only (no MSI), `release-app.yml`
  (manual, draft by default), SignPath when its secrets exist, updater feed
  `latest.json` in the `app-latest` release (only when `RK_UPDATER_PUBKEY`
  + private key are set), opt-in crash reports (local, redacted, GitHub
  issue opened by the player), release artifacts = ZIP with zstd entries.
- **0016** Backups: manual = player databases only (`manual-<secs>.sql`,
  kept); automatic `before-*` dumps pruned to the newest 5; restore =
  resumable `instance-<id>` op (full dump first, drop + replay only the
  player databases the dump holds with `mysql --one-database`, dbimport);
  `rk join <address>` / Home panel points the client at a LAN host.
- **0017** Realm settings = emulator manifest `settings` (sections of
  options, new `number` type), values under `realm` in `instance.json`,
  `User` layer of core files; driver copies `GameType` to `realmlist.icon`.
  `rk uninstall` (optional last backup, shared downloads removed with the
  last realm). Data root choosable before the first install
  (`location.json` in `%LOCALAPPDATA%\RealmKeeperData`; the picked folder itself since 0019).
- **0018** Beta readiness: free space checked before downloads, unpacking and
  a new install (catalog `unpackedSize`, emulator `instance.diskSize`);
  `disk.full`/`disk.fullNow`, `port.inUseBy` (netstat + tasklist);
  `rk diagnostic` / footer copy (redacted); `rk client find` (bounded
  scan); `EmulatorDriver::population` polled every 20 s; `daily-*` backups
  (keep 7) after a clean stop; `preferences.json` in the app dir
  (`dailyBackup`, `hideWhilePlaying`); tray icon; window state plugin +
  fit to the screen; release notes shown after a launcher update.
- **0019** The picked folder is the data root (new/empty or already a data
  root: marker `.realmkeeper`, `location.notEmpty`); a missing root is
  never recreated (`missing`, UI: locate it or use the default); another data
  root can be chosen at any time; a realm whose configs name another folder is
  `moved`: start refused (`instance.moved`), resume re-runs `modules-<id>`.
- **0020** Data safety: clean stop on Windows shutdown/log-off (shutdown
  block reason), power request while an operation works, data roots in
  synchronised or non-NTFS folders refused (`location.synced`/`unsupported`),
  quarantined binaries found from `.rk-binaries` (`server.filesMissing`).
  GM panel from manifest `consoleActions` (values checked by
  `rk_core::console`), start progress from process `startup` markers +
  "ready" notification when hidden, client add-ons as assets from a pinned
  repository archive (`addons` → `Interface/AddOns`, MultiBot). Several realms:
  `name` in the record, duplicate (cold copy, `bin`/`sql` hard-linked, new DB
  port), export `.rkrealm` (`realm.json`, `bindings.json`, player-data dump),
  import (install + restore + bindings, `import.json` resumable).
- **0021** Project name **RealmKeeper** (0.2.0): crates `rk-*`, CLI `rk`, env
  `RK_*`, identifier `io.github.realmkeeper.launcher`, launcher folder
  `%LOCALAPPDATA%\RealmKeeperData` (not `RealmKeeper`: the per-user installer
  puts the program there), marker `.realmkeeper`, exports `.rkrealm`, MySQL
  account `rk`. Logo = keep with a keyhole gate in a hexagon (`Logo.svelte`,
  `icons/logo.svg`); wordmark Cinzel + Cinzel Decorative R/K. No migration of
  0.1.x installs (start fresh). New index signing key (`RK_INDEX_SIGNING_KEY`):
  after a rotation CI keeps only the previous serial.
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
│  ├─ rk-core/              domain model, Catalog, resolver (pure, no I/O)
│  ├─ rk-catalog/           load catalog from disk + cross-manifest validation
│  ├─ rk-conf/              lossless .conf parser + layered renderer (pure, no I/O)
│  ├─ rk-db/                portable MySQL instance: init, start/stop, accounts, backup/restore
│  ├─ rk-store/             content-addressed store, parallel resumable sha256-verified downloads
│  ├─ rk-engine/            transactional step executor: journal, resume, rollback, atomic dir swap
│  ├─ rk-runtime/           supervisor: start order, readiness, console, logs, clean stop; driver trait
│  ├─ rk-client/            player's client: detection, realmlist, managed patch files, launch
│  ├─ rk-launcher/          player actions for app + CLI: errors, progress, sessions, crash reports
│  └─ rk-cli/               `rk` binary (CI, support, power users)
├─ drivers/
│  └─ azerothcore/           AzerothCore driver: accounts (SRP6), realm list
├─ app/
│  ├─ src/                   Svelte 5 UI (lib/api.ts, lib/i18n/)
│  └─ src-tauri/             thin Tauri adapter (commands.rs)
├─ tools/
│  ├─ validate-catalog.mjs   JSON Schema + layout validation of the catalog
│  ├─ check-i18n.mjs         FR/EN key and placeholder parity
│  ├─ test-guard.mjs         regression cases for the shell guard hook (npm run test:hooks)
│  ├─ ci/cdp.mjs, ci/ui-smoke.mjs   DevTools driver + UI smoke test (npm run test:ui)
│  └─ git-hooks/             pre-commit / commit-msg logic (Node, cross-platform)
├─ .githooks/                git hook entry points (core.hooksPath, set by npm install)
├─ .github/workflows/
│  ├─ ci.yml                 Windows full checks + Linux portable crates
│  ├─ release-app.yml        launcher installer: build, SignPath, update feed, release (manual)
│  └─ build-server.yml       phase 1: full server build + package + smoke test (on demand / catalog
│                            changes; reuses the artifact when the build key is unchanged)
├─ tools/ci/                 build-plan.mjs, package-server.mjs, smoke-test.mjs, index.mjs,
│                            zip-zstd.mjs (release archives), app-release.mjs (launcher release)
├─ docs/                     architecture.md, roadmap.md, decisions/ (ADRs), faq.md + faq.fr.md
│                            (player help, anchors linked from the UI errors)
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


`rk-db` integration tests need a real MySQL: set `RK_MYSQL_BASEDIR` to an
unpacked MySQL 8.4 ZIP (`tools/ci/fetch-mysql.ps1` downloads a verified one).

## 7. Commands

```sh
npm install                                   # JS deps (root + app workspace)
npm run dev                                   # Tauri dev mode (needs Rust)
npm run check                                 # svelte-check + catalog schema + i18n parity
npm run validate:catalog                      # catalog JSON Schema + layout only
npm run check:i18n                            # FR/EN locale parity only
npm run test:ui                               # UI smoke test (debug app built with the ui-smoke.mjs config)
cargo test --workspace                        # Rust tests, incl. the real catalog
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p rk-cli -- catalog validate
cargo run -p rk-cli -- catalog resolve --emulator azerothcore --module mod-ah-bot
cargo run -p rk-cli -- store fetch --url <url> --size <bytes> --sha256 <hex>   # into .local/store
cargo run -p rk-cli -- component install mysql-8-4   # engine: download + atomic extract into .local/rk
cargo run -p rk-cli -- install solo [--index <file|url>]   # from the signed index (default: published one)
cargo run -p rk-cli -- install solo --server <artifact.zip> [--db-snapshot <snapshot.zip>]   # local files
cargo run -p rk-cli -- index show [<file|url>]   # verify and print the index
cargo run -p rk-cli -- start <id>            # foreground: console on stdin, Ctrl+C / .stop = clean stop
cargo run -p rk-cli -- account create <id> <name> [--password <p>] [--gm 3]
cargo run -p rk-cli -- module enable|disable <id> <module>...   # servers stopped
cargo run -p rk-cli -- update <id> [--server <artifact.zip>]   # new catalog release; dumps every DB first
cargo run -p rk-cli -- repair <id>           # reinstall server files + configs of the current release
cargo run -p rk-cli -- uninstall <id> [--no-backup]   # remove a realm (ADR 0017)
cargo run -p rk-cli -- resume <id>           # finish an interrupted install/update/repair/module change, or fix a moved realm's paths
cargo run -p rk-cli -- option set|show <id> [<module>|realm <option>=<value>...]   # servers stopped
cargo run -p rk-cli -- client set <game folder> [--locale frFR] | show | find | prepare <id>
cargo run -p rk-cli -- play <id> [--keep-running]   # prepare client, start servers, launch the game
cargo run -p rk-cli -- backup list|create <id> | restore|delete <id> <name>   # player-data backups (ADR 0016)
cargo run -p rk-cli -- join <address>        # point the client at a LAN host and launch the game
cargo run -p rk-cli -- diagnostic            # support report (redacted), ADR 0018
cargo run -p rk-cli -- realm duplicate <source> <id> [--name <n>] | export <id> <file> | import <file> <id> [--name <n>] | rename <id> [<name>]   # ADR 0020
node tools/ci/index.mjs keygen|entry|update|verify   # CI side of the index (ADR 0010)
node tools/ci/zip-zstd.mjs <dir> <out.zip> [--level 19]   # release archive (ADR 0015)
```

Environment (2026-11-13): Windows 11, PowerShell + Git Bash, Rust 1.100 (rustup,
MSVC target), Visual Studio 2022 Build Tools (MSVC 14.44, Windows SDK 10.0.26100),
Node 24, git, gh (authenticated). In a fresh shell, if `cargo` is not found,
prepend `%USERPROFILE%\.cargo\bin` to `PATH`. Build Tools reinstalled
2026-11-15 (C++ workload, MSVC 14.44, SDK 10.0.26100). Run cargo from
PowerShell: in Git Bash, `/usr/bin/link` shadows MSVC's `link.exe`.

Release builds: `npm run build` (per-user NSIS; signed releases only from
`release-app.yml`). The app uses `RK_ROOT` when set (e.g. `.local/rk` to
see the dev instances). Quick local check without
installers: `cd app && npx tauri build --debug --no-bundle`.

## 8. Conventions

### Catalog
- File name == `id` (kebab-case); flavors/modules live under their emulator dir.
- Pin full 40-char commits; `ref` is informational.
- `deny_unknown_fields` everywhere: **change `catalog/schema/*.json` and
  `crates/rk-core/src/manifest.rs` together**, then run both validators.
- A flavor's `requiredModules` must be `removable: false`; removable modules use
  `activation.mode: "config"`.
- Use the `add-module`, `add-emulator` and `bump-pins` skills for catalog work.

### Rust
- `#![forbid(unsafe_code)]` in library crates. The app crate denies it too,
  except `app/src-tauri/src/os.rs` (Win32 calls, each with a SAFETY comment;
  window calls run on the main thread). Errors: `thiserror` in libraries,
  `anyhow` only in binaries.
- `rk-core` stays pure (no fs/net/process). I/O goes in dedicated crates.
- Deterministic outputs (`BTreeMap`/`BTreeSet`, sorted directory reads).
- Tauri layer is an adapter: no business logic in `app/src-tauri`.
- Tests against the real catalog live in `crates/rk-catalog/tests/`.

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
- Repository: `github.com/FayneDre/realmkeeper` (public), default branch `main`.
- Use the `commit` skill. Conventional Commits, English, no trailers.
- Commit only when the maintainer asks. **The maintainer decides every push**:
  a commit request is not a push request; never push on your own initiative
  (not even to trigger CI). Never `--no-verify`, never force-push.
- Commit identity is repository-local (GitHub handle + no-reply e-mail); don't change it.
- Never commit build outputs, installed servers, client files or secrets.
- Architecture changes need an ADR in `docs/decisions/` (`new-adr` skill).

### Licensing
- Launcher: **GPL-3.0-or-later** (`LICENSE`). Upstream emulators/modules keep
  their own licenses (AzerothCore GPL-2.0); manifests record SPDX ids.

## 9. Current status

See `docs/roadmap.md` (2026-11-14). Phase 0 done. Phase 1 exit reached: the
server build workflow compiles the Playerbot fork with every catalog module,
reuses artifacts by build key, and the smoke test makes a database snapshot
(ADR 0009) and is green on its restore (modules off/on); a publish job releases
the build and signs the index (ADR 0010) once `RK_INDEX_SIGNING_KEY` is set;
remaining: zstd.
Phase 2 started: `rk-store`, `rk-conf`, `rk-db` and the `rk-engine`
executor are done, with the install steps and `rk install` (ADR 0007/0008),
verified end to end locally (servers start), and `--db-snapshot` restore
(ADR 0009) and `--index` (signed index, ADR 0010).
Phase 3 started (2026-11-15): `rk-runtime` supervisor and the AzerothCore
driver (`drivers/azerothcore`): `rk start <id>` (console, clean stop) and
`rk account create` verified locally; the maintainer logged in and played
(phase exit). Module lifecycle: `rk module enable|disable` with post-install
steps (AHBot account + character, `bindings.json`) and world regeneration on
removal of a world-changing module (`RENAME TABLE` aside + dbimport), and
`rk update` (new build: full dump, swap, dbimport; rollback restores the
dump), verified. Instance lock shared by start / module / update. Phase 3
done.
Phase 4 (2026-11-15): `rk-client` (detection by version resource,
realmlist + `Config.wtf`, cache, managed patch files), module options
(`rk option set`) selecting upstream assets (Individual Progression
patch-V/S, J/U, classic DBCs) with the server data overlay, `rk client
set|show|prepare` and `rk play` (ADR 0013). Next: phase 5 (public app). UI: left
rail of realms, Home (real "not installed" states) and Server settings
(modules) tabs; Account, Bots and Backups are placeholders until their phases.
Phase 5 (2026-11-15): `rk-launcher` (ADR 0014) shared by the CLI and the
app; UI wired to real states (install wizard, Play/start/stop, consoles,
modules, options, accounts, resume, update, Repair; verified by driving the
app over CDP on `pubtest`); `rk repair|resume`; per-user NSIS, release
workflow with SignPath + signed update feed, opt-in crash reports, zstd
release archives (ADR 0015). Updater key pair set (2026-11-15, private key
kept by the maintainer outside the repo); first `release-app.yml` run
published the draft `app-v0.1.0`. Remaining: SignPath project (maintainer),
closed beta.
After phase 5 (2026-11-15): Bots tab (Playerbots options, verified keys;
options of mandatory modules now listed and settable), Backups tab and
`rk backup` (ADR 0016, restore verified on `pubtest`), `rk join` / LAN
join panel, stop timeout counted from the last output line, `AlreadyExited`
stop outcome. Beta readiness and player comfort (ADR 0018, 2026-11-15):
free-space checks, port owners, diagnostic report, client auto-detection,
live players/bots, daily backups, tray icon, window state.
Install location used as picked; renamed or moved data roots and realms
recovered (ADR 0019, `pubtest` renamed and relocated by `rk resume`).
ADR 0020 (2026-11-16): data safety, GM panel, start progress, MultiBot
add-on, several realms per game version (name, duplicate, export, import;
verified on `pubtest` → `pubcopy` and `pubimport`).
ADR 0021 (2026-11-16): renamed to RealmKeeper 0.2.0 (logo, wordmark, `rk`
names, repository renamed, new index signing key and `RK_*` secrets). The
index is republished by the next `build-server` publish (new key).

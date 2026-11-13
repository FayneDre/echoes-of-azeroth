# Architecture

Echoes of Azeroth is a Windows-first desktop launcher that installs, runs and
updates World of Warcraft server emulators, their modules, and launches the
player's own game client. AzerothCore (with Playerbots) is the first supported
emulator; the architecture is built so that other emulators, clients and modules
are added as **data** (manifests) plus a small **driver**, not as rewrites.

Decisions behind this document are recorded in [`decisions/`](decisions/).

---

## 1. Principles

1. **No compilation on the user's machine.** CI builds everything; the launcher
   downloads signed, hash-verified artifacts.
2. **Generic engine, emulators as data.** Manifests describe what to install;
   drivers implement only what cannot be declared (accounts, console, config format).
3. **Content-addressed everything.** One store keyed by hash gives caching,
   deduplication, resumable downloads, differential updates and integrity.
4. **Transactional operations.** Every step is idempotent and resumable; installs
   are atomic (stage then swap); every migration is preceded by a backup and can
   roll back.
5. **Zero in-game compromise.** Optimisation targets the *installation*
   (speed, size, reliability). Server settings are never throttled.
6. **Isolation, no admin rights.** Each instance owns its binaries, database,
   config and logs under `%LOCALAPPDATA%`. No Windows service. The only elevated
   action is an optional firewall rule for LAN play.
7. **The client is never distributed.** The user points to their own client.

## 2. Domain model

```
Emulator ──< CoreFlavor ──< FullBuild (one per catalog release, all modules compiled in)
   │             │
   │             └──< Module (declares compatible flavors)
   └── ClientSpec (expected client version/build)

Instance  = Emulator + CoreFlavor + FullBuild + enabled Modules + MySQL instance
            + layered config + network profile (solo | LAN)
            └── linked to 1..n ClientInstall
ClientInstall = user-provided client folder + launcher-managed patches/realmlist
Component     = shared third-party runtime (MySQL 8.4 LTS, VC++ redistributable)
```

| Concept | Example | Notes |
|---|---|---|
| Emulator | `azerothcore` | Has a driver, a client, components, databases, processes, ports. |
| CoreFlavor | `playerbot` | A concrete source tree. Playerbots **requires** the `mod-playerbots/azerothcore-wotlk` fork (`Playerbot` branch). |
| Module | `mod-ahbot` | Optional unless listed in the flavor's `requiredModules`. |
| ClientSpec | `wow-3-3-5a` | Detection rules, realmlist format, patch directory, matching server data. |
| Component | `mysql-8-4` | AzerothCore requires MySQL 8.4 LTS (MariaDB unsupported since Sept 2024). |
| Instance | "My solo realm" | Fully independent; a user may have several. |

Manifests live in [`catalog/`](../catalog/) and are validated by JSON Schema
(`catalog/schema/`) and by the Rust `eoa-catalog` crate (cross-references).

## 3. Module strategy: the "full build"

AzerothCore modules are C++ compiled **into** `worldserver`. Because the user
can add or remove modules at any time, building one binary per combination
(2ⁿ builds, 30–60 min each) is not viable. Instead:

- CI produces **one full build per flavor and catalog release**, with every
  catalog module compiled in.
- The launcher **installs** a module by: applying its activation config,
  rendering its config files, allowing its SQL in dbimport
  (`Updates.AllowedModules`, confirmed present in the Playerbot fork's
  `dbimport.conf.dist`), running dbimport, running `postInstall` steps,
  restarting the server.
- The launcher **removes** a module by: applying its `disable` config,
  regenerating the `world` database (content, not player data) from the build's
  snapshot and re-applying the remaining modules' SQL. `playerData` databases
  (auth, characters, playerbots) are never regenerated; module tables there are
  left inert (optional purge).
- Every module manifest declares an `activation` mode. CI must prove that a
  `config`-mode module, compiled in but disabled, is inert.
- `removable: false` modules (Playerbots) cannot be removed.

Modules outside the catalog will require a local build (advanced users, later).

### How module SQL is controlled (verified in the fork's source, 2026-11-13)

- dbimport reads module SQL from `<SourceDirectory>/modules/<dir>/data/sql/<db-dir>/`,
  where `<dir>` is the module's **directory name** in `modules/` and `<db-dir>`
  contains `auth`, `characters` or `world` (e.g. `db-world`). `Updates.AllowedModules`
  takes those directory names. CI clones each module into `modules/<catalog id>`,
  so `dbimportId` == catalog id.
- `worldserver` also auto-applies SQL for **all compiled modules** on startup.
  Instances therefore set `Updates.EnableDatabases = 0` in `worldserver.conf`
  and the launcher runs dbimport itself with the instance's allowed modules.
- The Playerbots database is updated by worldserver itself
  (`Playerbots.Updates.EnableDatabases`); Playerbots is mandatory so this is fine.
- The server artifact ships the SQL tree (`sql/data/sql`, `sql/modules/<id>/data/sql`)
  and instances point `SourceDirectory` at it; dbimport needs `MySQLExecutable`.

### Instance working directory (Windows)

AzerothCore on Windows resolves its config directory as `configs/` relative to
the **working directory**, and module configs as `configs/modules/<file>.conf`.
Server processes always run with cwd = the instance root, which contains `configs/`.

## 4. Build & distribution pipeline (CI)

```
Implemented in `.github/workflows/build-server.yml` (scripts in `tools/ci/`):

catalog release ─► GitHub Actions (Windows runner)
                    ├─ checkout flavor@commit + all modules@commit
                    ├─ MSVC Release build (vcpkg + sccache caches)
                    ├─ dbimport on MySQL 8.4 → database snapshot
                    ├─ smoke tests: start, login, bots spawn, AH posts, module toggles inert
                    ├─ zstd packaging + per-file hash manifest
                    └─ sign (ed25519) + publish + update the catalog index
```

| Artifact | Frequency | Content |
|---|---|---|
| `server-<flavor>-<release>` | per catalog release | worldserver, authserver, dbimport, DLLs, `.conf.dist` |
| `dbsnapshot-<flavor>-<release>` | per catalog release | databases pre-populated by dbimport |
| server data | rare | **not ours**: downloaded from the pinned upstream release (`wowgaming/client-data`, ~1.1 GB, sha256-verified), ADR 0006 |
| `mysql-8-4` | rare | MySQL portable, trimmed to server + admin tools |
| module assets | per module release | e.g. Individual Progression MPQ patches / DBC overrides |

Hosting: GitHub Releases first (public repository, free; 2 GB per file). The signed index lists **several mirrors per artifact**
so Cloudflare R2 (or others) can be added without changing the launcher.

Licensing: AzerothCore and the fork are GPL-2.0; distributed binaries link to
the exact source commits recorded in the manifests.

## 5. Launcher architecture

Tauri 2 (Rust) + Svelte 5 UI. The Rust core is a set of libraries independent
of Tauri, reusable by the `eoa` CLI (CI, support, power users) and testable.

| Crate | Status | Responsibility |
|---|---|---|
| `eoa-core` | ✅ | Manifest types, `Catalog`, module resolver. Pure, no I/O. |
| `eoa-catalog` | ✅ | Load catalog from disk, cross-manifest validation. Later: signed remote index. |
| `eoa-cli` | ✅ | `eoa catalog validate`, `eoa catalog resolve`, `eoa store fetch`. |
| `eoa-store` | ✅ | Content-addressed store: parallel ranged downloads (8 × 16 MiB segments by default), resume from persisted segment state (also across mirrors), mirror fallback (also when a mirror serves corrupted bytes), size + sha256 verification before a blob becomes visible, OS file lock per blob (concurrent fetches download once), fail-fast on permanent HTTP errors. |
| `eoa-conf` | ✅ | Lossless `.conf` parser and layered renderer (§9). Pure, no I/O. |
| `eoa-engine` | planned | Step executor: journal, resume, rollback, atomic swap. |
| `eoa-db` | ✅ (first slice) | Portable MySQL instance: `my.ini`, secure init, start/readiness, clean stop, databases + service accounts, backup/restore (§8). Later: snapshot restore, Job Object. |
| `eoa-runtime` | planned | Process supervisor (Job Objects), readiness, logs, crash detection. |
| `eoa-client` | planned | Client detection, realmlist, managed patches, launch. |
| `drivers/azerothcore` | planned | AzerothCore specifics (see §6). |
| `app/src-tauri` | ✅ skeleton | Thin adapter: Tauri commands/events over the crates. |

### Install engine step vocabulary

`download` · `verify` · `extract` · `render-config` · `merge-config` ·
`db-restore-snapshot` · `db-import` · `run-tool` · `create-account` ·
`create-character` · `set-config` · `copy-client-file` · `overlay-server-file` ·
`register-firewall`. The vocabulary is closed and tested; emulator-specific
steps are delegated to the driver.

### Resolution

`eoa_core::resolve` turns a selection into a deterministic set: adds the
flavor's required modules and transitive `requires`, rejects incompatible
flavors, conflicts and contradictory core overrides, and orders dependencies
first. The UI shows the resulting plan before anything runs.

## 6. Emulator drivers

```rust
trait EmulatorDriver {
    fn render_config(&self, instance: &Instance) -> Result<ConfigSet>;
    fn create_account(&self, ctx: &Ctx, user: &str, password: &str, gm: u8) -> Result<AccountId>;
    fn create_character(&self, ctx: &Ctx, spec: &CharacterSpec) -> Result<CharacterGuid>;
    fn set_realm_address(&self, ctx: &Ctx, address: &str) -> Result<()>;
    fn console(&self, ctx: &Ctx, command: &str) -> Result<String>;
    fn shutdown(&self, ctx: &Ctx) -> Result<()>;
}
```

Adding an emulator = manifests + one driver. Download, database, client,
runtime, UI and updates are reused unchanged.

## 7. On-disk layout

```
%LOCALAPPDATA%\EchoesOfAzeroth\
├─ store\                 blobs by hash (shared cache)
├─ components\mysql-8-4\
├─ instances\<id>\        bin\ sql\ data\ configs\ db\ logs\ instance.json  (cwd of the servers)
├─ clients\<id>\          metadata + managed patches (the client stays where it is)
├─ backups\<id>\
└─ state\                 operation journal, locks
```

Server data is shared from the store (hard links / junctions); files overridden
by module assets (e.g. `dbc/Spell.dbc`) are per-instance overlays.

## 8. Database (MySQL 8.4 LTS, portable)

- One MySQL instance per realm, dedicated auto-picked port, bound to `127.0.0.1`.
- Initialised by restoring the CI snapshot (seconds instead of minutes of dbimport).
- **Credentials:** root gets a random 32-character password during bootstrap
  (`--initialize-insecure` + `--init-file`, so the server never listens without
  one); service accounts (`acore`) get their own random passwords. They live in
  files under the user's profile (`client.cnf` for the MySQL tools, never on a
  command line). DPAPI would add nothing: upstream servers read their DB
  password in clear text from their `.conf` files. Protection = `127.0.0.1`
  bind + per-user profile ACL.
- `my.ini` (rendered by `eoa-db`): `mysqlx=OFF` (no port 33060 shared between
  realms), `disable-log-bin` (no replication: binary logs only cost disk and
  import speed), utf8mb4, durable `innodb_flush_log_at_trx_commit=1`, buffer
  pool 1 GiB and redo log 512 MiB by default (performance, not minimised).
- "Running" means an **authenticated** query succeeds, so another MySQL on the
  same port is never mistaken for the instance; a busy port is reported as such.
- Clean shutdown only (`mysqladmin shutdown`, wait until the port is free).
  Planned: Job Object (orphans), refuse datadirs inside cloud-synced folders.
- Backups: `mysqldump --single-transaction` (routines, triggers, events), written
  atomically; restore replays them. Covered by a real-MySQL lifecycle test
  (`EOA_MYSQL_BASEDIR`, run in Windows CI).
- Automatic backups before every migration and on a schedule.
- Optional "external database" mode for advanced users (MySQL 8.4 required).

## 9. Configuration layering

```
.conf.dist (from the build, upstream comments kept)
  + ModuleDefault   module manifest defaults                editable
  + User            UI options, advanced editor, hand edits  editable
  + CoreOverride    values a module requires in core files   locked
  + Launcher        paths, ports, DB credentials,            locked
                    Updates.EnableDatabases = 0
  ──────────────── eoa_conf::render ───────────────►  effective .conf
```

- Layers apply by kind (order above), then in the given order within a kind.
  `render` returns, for every managed key, its **provenance** (which layer and
  source won, and which values it shadowed), so the UI can show *why* a value is
  locked ("required by Individual Progression").
- The effective file is **regenerated**, never patched: after an update of the
  `.conf.dist`, new upstream keys and documentation appear automatically. Keys
  absent from the dist are appended under a marked section.
- **Hand edits survive:** before re-rendering, `manual_changes(last_written,
  on_disk)` detects keys the player edited or added in the file and stores them
  as `User` values (spacing/quoting differences are ignored; a deleted line means
  "back to the upstream default").
- Verified lossless on the fork's `worldserver.conf.dist` (592 keys),
  `authserver.conf.dist` and `playerbots.conf.dist` (897 keys):
  `EOA_CONF_SAMPLES=<files> cargo test -p eoa-conf --test upstream`.

## 10. Runtime

- Start order MySQL → authserver → worldserver, each gated by readiness
  (TCP port or log line). Stop in reverse with graceful shutdown.
- Integrated GM console, streamed and filterable logs, export for support.
- Crash detection, optional auto-restart, crash report.

## 11. Clients

- User selects a folder; the launcher checks `Wow.exe` build (12340 for 3.3.5a)
  and locale. The client is never distributed.
- Writes `Data/<locale>/realmlist.wtf` for the chosen instance (restorable).
- Module client assets (MPQ patches) are installed, tracked and removed cleanly.
- "Play" starts the instance if needed, waits for readiness, launches the client.
- Optional client tweaks (e.g. Large Address Aware) are opt-in with a backup.

## 12. Security & distribution

- Trust chain: signed index → per-artifact hash → verified before execution.
- Authenticode signing via SignPath Foundation (open-source program).
- Tauri updater with its own signing key.
- MySQL and SOAP bound to localhost; LAN mode exposes only auth/world ports.

## 13. Network scope

v1: solo (`127.0.0.1`) and LAN. Internet play (port forwarding help, account
hardening) is v1.x.

## 14. Open questions (tracked)

- ~~Exact `Updates.AllowedModules` naming~~ — resolved: module directory name (see §3).
- Creating the AHBot character without a client (driver command vs SQL template) — prototype in phase 1.
- Whether each catalog module is truly inert when compiled in and disabled — CI test.

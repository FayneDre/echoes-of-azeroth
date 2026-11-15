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
| Module | `mod-ah-bot` | Optional unless listed in the flavor's `requiredModules`. |
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
  restarting the server (§5.3).
- The launcher **removes** a module by: applying its `disable` config,
  regenerating the `world` database (content, not player data) with dbimport
  from the base SQL of the build and re-applying the remaining modules' SQL
  (§5.3). `playerData` databases
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

Implemented in `.github/workflows/build-server.yml` (scripts in `tools/ci/`):

```
catalog release ─► GitHub Actions (Windows runner)
                    ├─ checkout flavor@commit + all modules@commit
                    ├─ MSVC Release build (vcpkg + sccache caches)
                    ├─ dbimport on MySQL 8.4 → database snapshot
                    ├─ smoke tests: start, login, bots spawn, AH posts, module toggles inert
                    ├─ zstd packaging + per-file hash manifest
                    └─ sign (ed25519) + publish + update the catalog index
```

**Avoiding rebuilds.** Compiling takes about an hour, so the workflow compiles
only when the binaries would differ:

- On push it runs only for files that affect what is built or tested (catalog
  emulators, components, clients, `tools/ci/`, the workflow). Thanks to the key
  below, a smoke-test-only change costs ~20 min instead of a rebuild.
- A *plan* job computes the **build key**: a hash of the core and module repos
  and commits (whole plan entries), the Windows database client libraries, the
  global `env` and `build` job of the workflow, and the scripts the build runs
  (`build-plan.mjs`, `package-server.mjs`, `fetch-mysql.ps1`). If an
  unexpired artifact with that key exists from a run on `main`, the build job
  is skipped and the smoke test runs on that artifact. Smoke-test-only edits keep
  the key, and so do comment lines. `force-build` compiles anyway (e.g. runner image changed OpenSSL).
- Only artifacts from this repository's `build-server` workflow, produced by a
  push or manual run on `main`, are reused (`tools/ci/find-artifact.sh`):
  artifact names are predictable, so a fork PR could otherwise plant one.
  Lookup errors fall back to compiling.
- Server artifacts are kept 90 days; reuse does not extend that, so an
  unchanged key is compiled again after 90 days.

| Artifact | Frequency | Content |
|---|---|---|
| `server-<flavor>-<release>` | per catalog release | worldserver, authserver, dbimport, DLLs, `.conf.dist` |
| `db-snapshot-<build key>` | per server build | MySQL data directory after dbimport with the base module set, `snapshot.json` (ADR 0009) |
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
| `eoa-catalog` | ✅ | Load catalog from disk, cross-manifest validation, signed artifact index (`index`: Ed25519 envelope, trusted keys, anti-rollback, selection by catalog pins, ADR 0010). |
| `eoa-cli` | ✅ | `eoa catalog validate`, `eoa catalog resolve`, `eoa store fetch`. |
| `eoa-store` | ✅ | Content-addressed store: parallel ranged downloads (8 × 16 MiB segments by default), resume from persisted segment state (also across mirrors), mirror fallback (also when a mirror serves corrupted bytes), size + sha256 verification before a blob becomes visible, OS file lock per blob (concurrent fetches download once), fail-fast on permanent HTTP errors. |
| `eoa-conf` | ✅ | Lossless `.conf` parser and layered renderer (§9). Pure, no I/O. |
| `eoa-engine` | ✅ (executor) | Transactional step executor (§5.1): durable journal, OS lock per operation, resume after crash or cancel, plan fingerprint, reverse-order rollback (itself resumable), repair state, finalize, `DirSwap` atomic directory replacement. Concrete steps come with their phase. |
| `eoa-db` | ✅ (first slice) | Portable MySQL instance: `my.ini`, secure init, start/readiness, clean stop, databases + service accounts, backup/restore, adoption of a restored snapshot (§8). Later: Job Object. |
| `eoa-runtime` | ✅ (first slice) | Supervisor (§10, ADR 0011): MySQL then the manifest processes in order, each gated by readiness; console (stdin of the `close-stdin` process), output to `logs/<process>.console.log` + broadcast, crash detection, clean stop in reverse order; kill-on-close Job Object for `terminate` processes. `EmulatorDriver` trait. |
| `eoa-client` | planned | Client detection, realmlist, managed patches, launch. |
| `drivers/azerothcore` | ✅ (first slice) | Game accounts written to the auth database like `AccountMgr::CreateAccount` (SRP6), realm 1 address/port before each start. Later: AHBot account + character, module post-install steps. |
| `app/src-tauri` | ✅ skeleton | Thin adapter: Tauri commands/events over the crates. |

### 5.1 Install engine (`eoa-engine`)

An **operation** (install, update, module toggle...) is an id plus an ordered
list of **steps** (`run`, `undo`, `finalize`). One id per thing being changed
(`instance-<id>`), so operations on an instance are serialised.

- **Journal** `state/<operation>.journal.json`: status of the operation and of
  every step, written atomically (temp + fsync + rename) *before* each next
  action; an OS lock (`<operation>.lock`) prevents two runs at once and dies
  with a crashed process.
- **Resume:** running the same plan again skips done steps and re-runs the
  interrupted one, which is told it resumes (steps are idempotent).
  Cancellation is cooperative and stops at a safe point. The plan
  **fingerprint** (step ids, kinds, parameters) must match: an unfinished
  operation is never continued with a different plan.
- **Rollback:** a failed step and every started one are undone in reverse
  order, journaled and resumable. If an undo fails the operation is
  `rollback-failed`: it blocks later operations until a retried abort succeeds
  or the instance is repaired.
- **Finalize:** once every step is done, steps drop what they kept for a
  rollback; a failed cleanup is reported, not fatal.
- **`DirSwap`:** replaces an owned directory (`bin/`, `sql/`) through a sealed
  staging copy and keeps the previous version until finalize, so the rollback
  of a *later* step can still restore it. Every crash point is recovered (an
  unsealed staging is never installed; both versions are never lost). An owner
  marker holds the execution's run id: leftovers of another execution are
  settled, never restored, so an undo cannot downgrade. Never
  used for player data.
- Step errors stay typed (`Box<dyn Error>`), so the UI can translate the
  underlying `StoreError`, `DbError`... (principle 8).
- Built-in steps (`eoa_engine::steps`): `download` (`Fetch`, into the store;
  cancellable, resumes from the store's segments) and `extract` (`Extract`:
  ZIP into a `DirSwap` staging directory, several threads largest files first,
  CRC-32 checked per entry, absolute / `..` / drive / link entries rejected
  before anything is written, optional strip of the single root directory).
  `eoa component install mysql-8-4` chains them: 281 MB download + 1.26 GB
  extraction in about 6 s on a fast connection (2026-11-14).

### 5.2 Installing an instance (ADR 0007)

`eoa install <id> [--index <file|url> | --server <artifact.zip> [--db-snapshot <snapshot.zip>]] [--module ...] [--lan]`.
Without `--server`, the signed index (default: the published one) is
accepted (`state/index.json` keeps the last one, anti-rollback), the build
matching the catalog pins is selected and `build-<key>` downloads its server
and, when usable, its snapshot into the store (ADR 0010). Then:

1. Shared operations, each with its own journal: `component-<id>` (MySQL)
   and `serverdata-<client>-<version>` (download + extract into
   `serverdata/`). An instance rollback never touches them.
2. `instance.json` + `secrets.json` are created once (modules, network,
   ports, MySQL port, service password), so a resumed install has the same plan.
3. `instance-<id>`: `extract-bin` (artifact `dist/`), `extract-sql`,
   `db-init` (refuses a database it did not create; undo deletes only its
   own), `db-start`, `db-provision` (databases + service account),
   `render-config` (every file of the full build, layered, swapped in
   atomically, reference copy in `configs/.eoa-rendered/`), `db-import`
   (`run-tool`: dbimport with cwd = instance root, log in `logs/`), `db-stop`.
   With `--db-snapshot`, `db-init` + `db-start` become `db-claim` (same
   rule), `extract-db` (snapshot `data/` → `db/data`) and `db-adopt`. The
   snapshot must match the artifact's build key, the MySQL version and the
   instance's dbimport modules, or the plan fails before any step (ADR 0009).
4. The module operation below (post-install steps of the chosen modules,
   final configs).

### 5.3 Modules on an existing instance

`eoa module enable|disable <id> <module>...` saves the new set in
`instance.json` (validated by the resolver; restored if the operation fails),
then runs `modules-<id>` (`eoa_runtime::modules`; its own journal, so an
install interrupted after `instance-<id>` resumes here):

```
servers-stopped → db-start → post-install → [regenerate-<db>]
  → render-config → db-import → db-stop
```

- `post-install` runs the manifest steps of the chosen modules through the
  driver, once: `create-account` (random password; the driver finds an
  existing account by name, so a crash never duplicates it) and
  `create-character`. Results go to `instances/<id>/bindings.json` per
  module (`ahbot_account.id`, `ahbot_character.guid`). It refuses to run
  while a server port is open. Rollback keeps what was created (player-data
  databases); re-enabling reuses it.
- `set-config` values are locked `Launcher` layers whose placeholders are
  expanded **at render time** from the bindings (the plan, and so its
  fingerprint, does not depend on ids the database chose). A value whose
  binding does not exist yet is left out: the key keeps its upstream default.
- Disabling applies the module's `disable` values; its bindings stay out of
  the configs. A module whose SQL is harmless while disabled
  (`applyWhenDisabled`) needs nothing more. For the others, every content
  database it `touches` (`playerData: false`, i.e. `world`) is listed in the
  record's `regenerate` (saved first, so a resumed operation has the same
  plan; cleared when it completes) and rebuilt (§3): `regenerate-<db>` moves
  all its tables into `<db>_eoa_previous` with one atomic `RENAME TABLE`
  (instant, 554 MB of tables), dbimport then rebuilds it from the base SQL
  and the remaining modules' SQL; finalize drops the old copy, rollback
  drops the rebuilt tables and moves the old ones back. Player databases
  keep the module's tables, inert.

Verified 2026-11-15 with AHBot on a local instance: account 104 + character
1002 created in 13.6 s, 249 auctions posted in each of the three houses on
the first cycle, re-enable `AlreadyCompleted`, disable back to the defaults.
Individual Progression: enable 70 s; disable with the world rebuild 2 min 05 s
(its tables gone, characters and AHBot auctions untouched, server starts).

### 5.4 Updating an instance

`eoa update <id> [--server <artifact.zip>] [--index <file|url>]` installs the
build of the current catalog release (core or module pins changed: the index
entry selected by the catalog pins, ADR 0010). The record keeps the build key
it runs (`build`); the same key is "already up to date". Op `instance-<id>`:

```
servers-stopped → db-start → db-backup → extract-bin → extract-sql
  → post-install → render-config → db-import → db-stop
```

- `db-backup` dumps **every** database (player data included) to
  `backups/<id>/before-update-<from>-to-<to>.sql` (atomic: `.partial`, then
  rename). That name exists only while the operation runs: finalize renames
  the dump `…-<unix seconds>.sql` (kept), and a successful undo deletes it
  once restored, so a dump found on resume is always this operation's own,
  never a stale one that a rollback would restore over later play.
- `db-import` migrates with the new SQL tree. Any failure swaps the old
  `bin/`, `sql/` and `configs/` back, drops each database and restores the
  dump; the record keeps the old build.

**Exclusion.** `eoa start`, `eoa module` and `eoa update` hold an OS lock on
`instances/<id>/instance.lock` (released on exit or crash), so nothing
changes an instance whose servers are running or still loading, and two
changes never overlap. A half-done `instance-<id>` blocks module changes and
starts, a half-done `modules-<id>` blocks updates and starts, until the
command that began it is run again (resume). `servers-stopped` also checks
the server ports, for servers started outside the launcher.

Verified 2026-11-15 on a local instance: update 28.6 s (dump 395 MB); same
build again "already runs"; an artifact without `dbimport.exe` fails at
`db-import` and rolls back in 80 s (binaries back, 1002 characters and 747
AHBot auctions intact, record unchanged); a real update then repairs it.

The config layers are a pure function of the catalog, the resolution and the
instance values (`eoa_engine::install::plan_configs`), tested against the real
catalog in `crates/eoa-catalog/tests/install.rs`.

### Install engine step vocabulary

`download` · `verify` · `extract` · `render-config` · `merge-config` ·
`db-init` · `db-start` · `db-provision` · `db-stop` ·
`db-claim` · `db-adopt` · `db-import` · `run-tool` · `create-account` ·
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
// eoa-runtime (ADR 0011); futures are boxed, errors typed for the UI.
trait EmulatorDriver: Send + Sync {
    fn id(&self) -> &str;                                   // manifest `driver`
    fn prepare_start(&self, cx: &DriverContext) -> DriverFuture<()>;   // DB up, servers not yet
    fn create_account(&self, cx: &DriverContext, a: &NewAccount) -> DriverFuture<u32>;
    fn find_account(&self, cx: &DriverContext, name: &str) -> DriverFuture<Option<u32>>;
    fn create_character(&self, cx: &DriverContext, c: &NewCharacter) -> DriverFuture<u32>; // servers stopped
    fn find_character(&self, cx: &DriverContext, name: &str) -> DriverFuture<Option<(u32, u32)>>;
}
```

`DriverContext` gives the running database, the emulator manifest, the
instance record and the realm address (`127.0.0.1` solo, LAN address in LAN
mode). Console commands, readiness and stop are declared in the manifest,
not in the driver. The AzerothCore driver writes characters as one
`characters` row with the next guid (mod-ah-bot never loads its character:
it builds an in-memory `Player` from the guid).

Adding an emulator = manifests + one driver. Download, database, client,
runtime, UI and updates are reused unchanged.

## 7. On-disk layout

```
%LOCALAPPDATA%\EchoesOfAzeroth\
├─ store\                 blobs by hash (shared cache)
├─ components\mysql-8-4\
├─ serverdata\<client>-<version>\   shared server data (ADR 0006)
├─ instances\<id>\        bin\ sql\ configs\ db\ logs\ instance.json secrets.json  (cwd of the servers)
├─ assets\<sha256>\       module assets, unpacked once per archive (ADR 0013)
├─ clients\<client>.json  registered client + files the launcher put in it (.lock while used)
├─ backups\<id>\          database dumps before updates
└─ state\                 operation journals, locks
```

Instances also hold `bindings.json`, `instance.lock` and, while a module
asset overrides a server file, `data\`. Server data is extracted once per
client version into `serverdata\` and shared through an absolute `DataDir`
(ADR 0007). While an override exists (e.g. `dbc/Spell.dbc`), `DataDir`
points to the instance overlay `instances\<id>\data\`: junctions to the
shared directories, `dbc\` rebuilt with hard links plus the overrides
(ADR 0013). The client itself stays where the player installed it.

## 8. Database (MySQL 8.4 LTS, portable)

- One MySQL instance per realm, dedicated auto-picked port, bound to `127.0.0.1`.
- Initialised by restoring the CI snapshot when one matches the server build
  (ADR 0009): `db-claim` (never over an existing database), `extract-db`,
  `db-adopt` (new root password through `--init-file`), then dbimport only
  checks and applies the chosen modules' SQL (~10 s instead of 6-14 min).
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

Implemented in `eoa-runtime` (ADR 0011), driven by `eoa start <id>`:

- Start order MySQL → driver `prepare_start` → manifest processes in array
  order (authserver, worldserver), each gated by readiness (TCP port or output
  line; a port already open before spawning is an error). Working directory =
  instance root; arguments from the manifest.
- Stop in reverse order with the declared method: `close-stdin` (worldserver
  saves and exits at end of input, also when the launcher dies) or
  `terminate` (stateless, kill-on-close Job Object). MySQL is stopped last
  with `mysqladmin shutdown`, never killed; left running if the launcher dies
  and adopted at the next start.
- Console: lines go to the stdin of the `close-stdin` process (GM console).
  Output of every process is streamed (broadcast) and written to
  `logs/<process>.console.log` (previous run kept as `.previous.log`).
- Crash detection: any process exit stops the others. Planned: optional
  auto-restart, crash report.

## 11. Clients

Implemented in `eoa-client` (ADR 0013); the client is never distributed.

- `eoa client set <folder>`: the executable's version resource must carry the
  manifest build (`Wow.exe` 3.3.5.12340); the locale comes from the
  `Data/<locale>` folders (asked when several). State in
  `clients/<client>.json`.
- Module options (`eoa option set <id> <module> <option>=<value>`) select
  assets; the module operation downloads them from the module's pinned
  commit (sha256-pinned `.zip` or `.7z`), unpacks them once and builds the
  server overlay (§7).
- Before each launch (`eoa play`, or `eoa client prepare <id>`):
  - `Data/<locale>/realmlist.wtf` and the `SET realmList` line of
    `WTF/Config.wtf` (when present) point at `127.0.0.1`;
  - the cache is cleared when another instance was played last;
  - the instance's client files (MPQ patches) are reconciled, tracked by
    sha256. A file of the player at the same place is moved to
    `.eoa-backup` and restored later, and a tracked file the player edited
    is never deleted.

  Each change is recorded first, so the next preparation finishes an
  interrupted one.
- `eoa play <id>` prepares the client and starts the servers. It launches the
  game from its folder once worldserver is ready, and stops the servers when
  the game exits (`--keep-running` keeps them up). A client lock prevents
  two sessions from rewriting the same client.
- Planned:
  - optional client tweaks (e.g. Large Address Aware), opt-in with a
    backup;
  - preparing a LAN client on another computer.

Verified 2026-11-15 with the maintainer's 3.3.5a client on a local instance:
- **Registration:** the client is registered (12340, enUS), and a 1.12.1
  folder is refused (build 5875).
- **Options:** Individual Progression options (patch-V, Vanilla visuals)
  apply in 15 s, including the 25 MB download. The overlay links the four
  big directories, and its `Spell.dbc` has the upstream sha256.
- **Play:** `eoa play` starts worldserver in 74 s on the overlay (112 data
  stores, no DBC error) and launches the game. Closing the game stops
  every server cleanly.
- **Back to the defaults:** the overlay and the three MPQs are removed, and
  the client's files and settings are byte-identical to before.

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

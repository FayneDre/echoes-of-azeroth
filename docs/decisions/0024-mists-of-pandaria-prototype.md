# 0024. Mists of Pandaria prototype: the second emulator

- Status: Accepted
- Date: 2026-11-19

## Context

ADR 0004 promises that another emulator comes as data plus a small
driver, never as a rewrite. Only AzerothCore has been tested so far. To check
that claim, we use a real second emulator with Playerbots, chosen by the
maintainer:
[1LineAtaTime/LoA-Pandaria-Playerbots](https://github.com/1LineAtaTime/LoA-Pandaria-Playerbots).

Upstream facts (GitHub API and README, 2026-11-19):

- The project is a fork of Legends-of-Azeroth Pandaria 5.4.8, GPL-2.0. Its default
  branch is `playerbot` (`f5fc970d`); release `v1.1.1` (2026-11-13) targets
  `2f9aebde`. The client is Mists of Pandaria 5.4.8, build **18414**, in the enUS
  locale (bots need the enUS DBC files).
- Requirements: MySQL "5.7 / 8.0-8.1", OpenSSL 1.1.1 or 3.0-3.1.1 (3.2
  unsupported), Boost ≥ 1.85, VS 2019 or later. The upstream Windows CI uses
  Boost **1.87**, OpenSSL 3.1.1 (choco) and the runner's MySQL **8.4**
  client library (`windows-build-release.yml`), with `cmake -DTOOLS=ON`.
- Releases ship binaries, the map extractors, a `database` zip and a `sql` zip.
  The `database` zip holds complete dumps for a new install: `auth`,
  `characters`, `world`, `playerbots`. The `sql` zip holds the sources of the
  built-in updater (`Updates.EnableDatabases`, a bit mask of 15 for the four
  databases, plus `SourceDirectory` and `MySQLExecutable`). The worldserver applies the
  updates itself; there is no dbimport.
- No game data is shipped. `mapextractor` writes `dbc/`, `maps/` and
  `cameras/`. `vmap4extractor` followed by `vmap4assembler` writes `vmaps/`.
  `mmaps_generator` writes `mmaps/`, which bots need everywhere, and
  "takes several hours".
- Playerbots settings use the prefix `AiPlayerbot.*`, in `playerbots.conf`
  next to `worldserver.conf`; `AiPlayerbot.Enabled = 0` turns them off.
- The auth schema is TrinityCore-like: `account` (`sha_pass_hash`, `v`,
  `s`, `battlenet_account`, `gmlevel`), `account_access`, `battlenet_accounts`,
  and `realmlist` (`gamebuild = 18414`, `icon`). `world.game_tele` has
  `Stormwind` and `Orgrimmar`; the highest `creature.guid` is 4 100 050.

Local test, 2026-11-19: the four `v1.1.1` dumps (sha256 `cb283195…`) import
with no error into a fresh **MySQL 8.4.13** (`--commands=OFF`), in 94 s
(world 79 s). They contain no client commands and no `DEFINER`.

MySQL 8.0 reached its end of life in April 2026.

## Decision

1. **Same rules as AzerothCore.** Our CI builds the server: one more flavor in
   `build-server.yml`, the commit pinned in the catalog, the result published in
   the signed index (ADR 0003, 0010). Upstream binaries are never installed:
   they are unsigned, made by one person, and outside our chain of trust.
   The build also produces the map extractors (`-DTOOLS=ON`), because they run
   on the player's machine.
2. **MySQL 8.4.** The existing `mysql-8-4` component serves both emulators.
   MySQL 8.0 (end of life) becomes a component only if the core fails on 8.4
   at start or in game. That would require a new ADR.
3. **Catalog.** Add a client `wow-5-4-8` (build 18414, enUS required), an
   emulator `pandaria` with flavor `playerbot`, and its process manifests and
   launcher-imposed instance config (ADR 0007). Playerbots is built in and
   switched by `AiPlayerbot.Enabled`. The first prototype has no separate
   modules.
4. **Databases.** For a new realm, the upstream `database` dumps are produced
   by our CI from the pinned commit and restored as a snapshot (ADR 0009).
   Updates run through the worldserver's built-in updater, with the SQL
   sources of the pinned commit. `characters`, `auth` and `playerbots` are
   player data (`playerData: true`).
5. **Game data from the player's client.** A new resumable install step runs
   the extractors on the player's own client. They write into the shared store,
   once per client build and locale, and never into the client folder. Progress
   comes from their output. mmaps come last, run with every core, and are
   resumable map by map where the generator allows it. Nothing extracted is
   ever uploaded or shared (principle 7).
6. **Driver.** `drivers/pandaria` implements `EmulatorDriver`: accounts in
   the TrinityCore 5.x form (to verify against the core's
   `AccountMgr`), the realm address and `gamebuild` before each start, and
   `spawns` through the same `game_tele` join (ADR 0023) if a module needs
   them later.

## Consequences

- If the engine, catalog schema or UI need emulator-specific code to make this
  work, we treat it as a defect of the abstraction and fix it in the generic
  layer, recording that in this ADR.
- The first install of a MoP realm takes hours (mmaps), on top of the
  download. The UI must show that before the player starts, and the step must
  survive a reboot (ADR 0014 `pending.json`).
- A second Windows build job; Boost 1.87 can share the AzerothCore cache.
  OpenSSL must stay at 3.1.x for this core.
- Still to verify, in this order: the CI build; the worldserver on MySQL 8.4
  (start, updater, auth); account creation and login with a 5.4.8 client;
  extraction time and size; bots online.
- Only a genuine 5.4.8 client is supported: detection checks the version
  resource of the game executable (build 18414) and the enUS data.

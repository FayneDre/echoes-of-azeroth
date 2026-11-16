# 0021. Project name: RealmKeeper

- Status: Accepted
- Date: 2026-11-16

## Context

The launcher had a working name, used up to 0.1.1, that turned out to be
taken several times: a cinematic of the game itself, an add-on, fan
projects. It also contained a Blizzard place name. A name without one fits
principle 7 better (nothing of Blizzard's).

The maintainer chose **RealmKeeper**. It says what the launcher does (it
keeps your realms) and is not tied to one game, which suits other emulators
later. A web search found no software of that name. The plural
"REALMKEEPERS" is a game trademark filed in 2024 by a mobile game publisher.
The maintainer was told and kept the name.

The 0.1.x releases had no players besides the maintainer, who reinstalls
from scratch.

## Decision

### Names

| What | Name |
|---|---|
| Product, window, installer | **RealmKeeper** |
| Wordmark | "RealmKeeper" in Cinzel, with the R and K in Cinzel Decorative 700 |
| Rust crates | `rk-*` (`rk-driver-azerothcore`) |
| CLI | `rk` |
| Environment variables, CI secrets | `RK_*` (`RK_ROOT`, `RK_INDEX_SIGNING_KEY`, `RK_UPDATER_PUBKEY`, ...) |
| App crate, executable | `realmkeeper` |
| Tauri identifier | `io.github.realmkeeper.launcher` |
| Repository | `FayneDre/realmkeeper` |
| Launcher folder | `%LOCALAPPDATA%\RealmKeeperData` |
| Data root marker | `.realmkeeper` |
| Realm export | `.rkrealm` |
| Internal suffixes | `.rk-*` (swaps, `.rk-binaries`, `.rk-backup`), `<db>_rk_previous` |
| MySQL account of a realm | `rk` |
| Signed index prefix | `realmkeeper/index/v1\n` |
| Version | 0.2.0 |

The launcher folder is not `RealmKeeper`. The per-user NSIS installer
puts the program in `%LOCALAPPDATA%\<product name>`, and sharing that folder
would put the realms inside the program folder, which an uninstall cleans.

**Mark.** A crenellated keep whose gate is a keyhole, inside a hexagonal
seal, drawn in gold:

- an original drawing, never a Blizzard symbol;
- `app/src/lib/Logo.svelte` holds the same paths as
  `app/src-tauri/icons/logo.svg`, the source of every application icon
  (`npx tauri icon`).

### No migration

Installs made by 0.1.x are not read: no former folder, marker, prefix or
file extension is recognised. They are not deleted either; the player
uninstalls them with the version that made them. The ADRs use the new
names throughout.

### Index signing key

With the fresh start, the index signing key is replaced too, by a new
Ed25519 key pair:

- its private key is in `RK_INDEX_SIGNING_KEY` and in the maintainer's
  offline copy of the secrets, outside the repository;
- its public key replaces the former one in
  `crates/rk-catalog/trusted-keys.json`.

`tools/ci/index.mjs update` handles a previous index signed with a key that
is no longer trusted (a rotation):

- it keeps only that index's serial, so serials keep increasing;
- it drops that index's builds, never re-signing what it cannot verify.

The index is published again, with the new key, by the next
`build-server` publish.

## Consequences

- Until that publish, launchers built with the new key find no valid index:
  installing a realm needs it.
- Players who ran 0.1.x (the maintainer only) uninstall it and start again
  with 0.2.0.
- The repository rename keeps the former URLs as GitHub redirects.
- A trademark search is still due before a public 1.0.

# 0026. Restoring a backup made by another server

- Status: Accepted
- Date: 2026-11-20

## Context

A restore (ADR 0016) replays the player databases of a backup, then runs
dbimport. Backups made by the launcher itself always worked. A backup that
the maintainer brought from another AzerothCore server (a Steam Deck
install, made with another tool) showed two gaps:

- The backup left out `acore_auth.realmlist`, on purpose: it is the realm's
  own address, not player data. Without the table, dbimport cannot prepare
  the login statements and aborts (`Table 'acore_auth.realmlist' doesn't
  exist`, then "Could not prepare statements of the Login database"). The
  launcher rolled back cleanly, but the restore was impossible.
- `bindings.json` keeps the ids of the accounts and characters that module
  post-install steps made (AHBot account and character, ADR 0008). After a
  restore these ids name other rows (here a random bot account), or
  nothing: AHBot would sell under a stranger's name, or not start.

Both also hit a restore of a backup from another RealmKeeper realm made with
another module set.

## Decision

1. **Realm tables.** A database of the manifest may list `realmTables`:
   tables that describe the realm, not its players. AzerothCore and LoA
   Pandaria list `realmlist` in their `auth` database. The `db-restore`
   step dumps these tables first (`db/restore-keep/`), replays the backup,
   drops whatever copy the backup had and puts the realm's own back. The
   saved copy survives an interruption, so a resume puts back the realm's
   tables, never half-restored ones.
2. **Bindings found again.** The restore operation is now
   `servers-stopped → db-start → db-backup → db-restore → db-import →
   post-install → render-config → db-stop`. Its post-install step looks
   every account and character up again by name in the restored data
   (created when missing, as on install) instead of trusting the ids in
   `bindings.json`; `render-config` then writes the module configs with the
   new ids. The previous `bindings.json` is kept aside and put back if the
   operation rolls back, since the databases go back too.
3. **Older builds.** A restore must work before a server update, when the
   catalog has a module the realm's build lacks. For the restore only,
   `render-config` keeps the current copy of a config whose `.conf.dist`
   the build does not have, instead of failing. Other changes still need
   the matching build.

## Consequences

- A backup from another AzerothCore server restores in one action, as long
  as it holds `USE` lines (a `mysqldump --databases` dump) and the
  `-- Dump completed` line.
- The AHBot account (`AHBOT`) of a restored backup is reused; its character
  is found by the catalog name (`Auctioneer`) or created on that account. A
  backup whose AHBot character has another name gets a new `Auctioneer`;
  auctions of the old character stay in the auction house until they
  expire.
- Accounts keep the names and passwords they had on the other server.
  Renaming an account is not part of a restore.

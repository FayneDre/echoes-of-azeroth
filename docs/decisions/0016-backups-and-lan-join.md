# 0016. Player-data backups, restore, and joining a LAN realm

- Status: Accepted
- Date: 2026-11-15

## Context

- **Backups.** Every update and repair already dumped every database to
  `backups/<id>/before-<label>-<secs>.sql` (ADR 0012, 0014). Nothing listed,
  pruned or restored them. On `pubtest`, one such dump is 395 MB, so the
  folder grows by that much at each update.
- **Player data versus content.** The databases with `playerData: true`
  hold everything a player cannot get back:
  - `acore_auth`;
  - `acore_characters`;
  - `acore_playerbots`.

  `acore_world` is rebuilt from the server build and the modules (ADR 0003,
  0012). On `pubtest`, a dump of the player databases alone is 72 MB, made
  in 11 s.
- **Selective restore.** `mysqldump --databases` writes a `USE` line before
  each database. `mysql --one-database <db>` replays only the statements
  for that database, so one database can be restored from a dump that holds
  several.
- **Database-level grants.** In MySQL, grants on a database survive a
  `DROP DATABASE`, so the service account keeps its access to a database
  that is dropped and recreated.
- **Joining a LAN realm.** The host already writes its LAN address into the
  realm list (ADR 0011) and shows it. A player on another computer still
  had to edit `realmlist.wtf` by hand.

## Decision

1. **Manual backups.** "Back up now" (`eoa backup create`) dumps the
   **player databases** to `manual-<secs>.sql`.
   - The servers must be stopped, which the instance lock enforces. The
     database is started for the dump and stopped afterwards.
   - Manual backups are kept until the player deletes them.
2. **Retention.** After each completed update, repair or restore, only the
   newest **5** automatic dumps are kept: the `before-…-<secs>.sql` files.
   - Manual dumps are never pruned.
   - A dump of an operation still in flight has no time suffix yet, so it is
     never pruned.
3. **Restore** (`eoa backup restore`, the Restore button) is an
   `instance-<id>` operation, recorded in `pending.json` as
   `Pending::Restore{dump, databases, label}` and resumable like an update:

   ```text
   servers-stopped → db-start → db-backup → db-restore → db-import → db-stop
   ```

   - `db-backup` dumps every database first (`before-restore-<secs>`). It is
     the rollback, and it lets the player undo the restore.
   - `db-restore` replaces only the **player databases that the dump
     holds**. It checks the dump's `USE` lines first, then for each database
     it drops it, recreates it and replays it with `--one-database`. A dump
     that lacks a database never empties it.
   - The content databases are left as they are. They match the current
     build and modules.
   - `db-import` migrates the restored databases to the current build.
   - A dump with no player database of the realm is refused
     (`backup.unusable`).
4. **Backups tab.** It lists the backups, newest first, with:
   - their kind, date and size, and the total disk use;
   - Back up now, Open the folder;
   - Restore, behind an explicit confirmation;
   - Delete.
5. **Joining a LAN realm.** The Home tab has a "Join a game on the local
   network" panel, and the CLI has `eoa join <address>`.
   - It points the registered client at the host's address and launches the
     game. No server runs on the joining computer.
   - The address must be an IPv4 address or a host name.
   - The files the launcher put in the client are removed, because the
     host's module patches are unknown here. The UI tells the player to copy
     the host's patch files when the host's options need them.

## Consequences

- **Module set.** A restore keeps the current module set. If a module's
  player tables changed since the dump, dbimport and the module bring them
  up to date. Characters whose items or quests came from a module that is
  now disabled stay inert, as on any module removal.
- **Dump size.** Automatic dumps stay full (every database), because their
  rollback needs the world too. Five of them take about 2 GB on a playerbot
  realm.
- **Not done yet:**
  - backups while the servers run (`mysqldump --single-transaction` would
    allow it, but the lock model forbids it today);
  - a size budget instead of a count;
  - pruning `assets/`;
  - a firewall rule for LAN hosting (principle 6).
- **Joining a realm that uses client patches** stays manual until a host
  can publish its client file list, for example from a later Internet-play
  phase.

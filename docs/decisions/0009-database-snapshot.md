# 0009. Database snapshot: cold copy of the MySQL data directory

- Status: Accepted
- Date: 2026-11-14

## Context

A new instance runs dbimport on empty databases: the core base SQL plus
about 3 300 update files (world alone: 1 085 released, 2 197 archived, each
applied through `mysql.exe`). Locally this takes 6 to 14 minutes, the slowest
step of an install. The result depends only on the server build (SQL files),
the MySQL version and the modules allowed in dbimport.

Measured locally on 2026-11-14 (MySQL 8.4.13, Playerbot fork build):

- A copy of a cleanly stopped data directory, without `auto.cnf`, `*.pem`,
  `#innodb_temp/` and the spare redo files (`#innodb_redo/*_tmp`), starts in
  about 2 s. MySQL generates a new server UUID, new TLS/RSA keys and the
  spare redo files. `--init-file` sets the root password before the server
  accepts connections.
- dbimport on databases that are already up to date takes 8.6 s. It records
  update names and hashes in `updates`, and `updates_include` paths are
  relative to `SourceDirectory` (`$/data/sql/...`): a copy works on another
  machine and in another directory.
- An active redo file is 16 MB. The spare files (512 MB with the launcher's
  `innodb_redo_log_capacity`) must not be shipped.

A logical dump (`mysqldump`) would still have to replay about 1 GB of SQL,
which takes minutes. InnoDB transportable tablespaces need every table to be
created first, then imported one by one.

## Decision

- **Format.** The snapshot is a physical copy of a MySQL data directory,
  taken after a slow shutdown (`innodb_fast_shutdown = 0`). It is made with
  the MySQL component of the catalog and the options that matter from the
  launcher's `my.ini` (no binary log, no X Protocol, utf8mb4).
- **Content.** It holds the state right after dbimport with the **base
  module set**: the flavor's mandatory modules plus the modules flagged
  `applyWhenDisabled` (ADR 0008). No server has run on it, so it holds no
  accounts, characters or bots. Its only accounts are MySQL's own: the
  dbimport user is dropped, and `root@localhost` has no password until the
  launcher's init file sets one. It has no `auto.cnf`, keys, certificates,
  temporary tablespaces, spare redo files, binary logs or error log.
- **Artifact.** CI produces `db-snapshot-<build key>` in the smoke-test job:
  `data/**` plus `snapshot.json` (`format`, `buildKey`, MySQL component
  `id`/`version`/`sha256`, `databases`, the base `modules`). The smoke test
  then restores it the way the launcher does and runs both rounds on it.
- **Restore** (`rk install --db-snapshot`) only creates a new database. It
  never replaces an existing one (`db-claim` refuses a database that this run
  did not create). The steps are `db-claim`, `extract-db` (archive `data/` →
  `db/data`, atomic), then `db-adopt`. `db-adopt` writes `my.ini` and
  `client.cnf` with a new random root password, then starts the server once
  with `--init-file` to set that password. Then, as usual, `db-provision`
  creates the service account and dbimport applies whatever the instance
  allows beyond the base set (e.g. Individual Progression's SQL), in seconds.
- **Matching.** A snapshot is accepted only if its `buildKey` equals the
  server artifact's, its MySQL version equals the catalog component's, and its
  modules are all allowed by the instance's dbimport list. Otherwise the plan
  fails with an explicit error. Without a snapshot, the install runs dbimport
  on empty databases, as before.

## Consequences

- An install takes seconds of database work instead of minutes, and
  regenerating `world` (ADR 0003) can restore the world tables from the same
  snapshot.
- The snapshot is tied to one MySQL version, so a MySQL pin bump needs a new
  snapshot. The CI job makes one on every smoke-test run.
- Measured locally (2026-11-14): 1.1 GB unpacked, 144 MB as a deflate ZIP.
  `rk install --db-snapshot` takes 17 s in all, against 6-14 min, and the
  servers start on the result. Compression (zstd), signing and the index
  entry follow the server artifact (phase 1 remainder).
- The empty root password exists only inside the archive. The server only
  ever starts on it with the init file, bound to `127.0.0.1`.
- A newer MySQL patch release could upgrade a snapshot's dictionary in place.
  This is not relied on: matching requires the same version.

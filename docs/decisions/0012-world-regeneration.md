# 0012. World regeneration: move the tables aside, rebuild with dbimport

- Status: Accepted
- Date: 2026-11-15

## Context

ADR 0003 regenerates the `world` database when a module that changed it is
removed. Module SQL cannot be undone: upstream modules ship no revert scripts,
and Individual Progression rewrites hundreds of world rows. The architecture
first said "from the build's snapshot", but the snapshot (ADR 0009) is a
cold copy of the whole MySQL data directory. One database cannot be taken
out of it without stopping MySQL and swapping InnoDB files.

Measured on a local instance on 2026-11-15:

- `acore_world` holds 316 InnoDB tables and 2 MyISAM tables (554 MB), one
  foreign key, and no triggers, routines or events.
- dbimport rebuilds an emptied `world` from the build's base SQL, plus the
  allowed modules' SQL, in 1.5 to 2 minutes (`Updates.AutoSetup` populates
  empty databases).

## Decision

- **Removal schedules a rebuild.** Removing a module that is not
  `applyWhenDisabled` lists every content database it `touches`
  (`playerData: false`) in the instance record's `regenerate` field. The
  record is saved before the operation runs, so a resumed operation has the
  same plan. The field is cleared when the operation completes. Player
  databases are never rebuilt: the module's tables stay there, inert.
- **Step `regenerate-<db>`**, placed between post-install and render-config:
  1. One atomic `RENAME TABLE` moves every table into `<db>_rk_previous`,
     with foreign key checks off. This takes no time, whatever the size.
  2. The following dbimport rebuilds `<db>` from the base SQL and the
     remaining modules' SQL.
  3. Finalize drops the old copy. It starts MySQL for this if needed, then
     stops it again.
  4. Undo drops what dbimport wrote, moves the old tables back and drops the
     empty copy. It starts MySQL first, because a rollback may resume after
     a restart.
- **Resuming.** The two schemas show how far the step got:
  - old copy full and `<db>` empty: the move is already done;
  - old copy empty: do the move;
  - both full: a copy left by a completed run whose cleanup failed; it is
    older than the live tables, so it is dropped and the move is done again.
- **Same command, same plan.** Repeating `rk module disable` resumes the
  operation: a module that is already off schedules nothing new.

## Consequences

- No CI change and no extra download; the build's SQL tree is enough.
- A removal costs 1.5 to 2 minutes, and needs free disk space for a second
  copy of the world tables (~0.6 GB) until finalize.
- Tested end to end:
  - Individual Progression removed: its tables are gone, while characters
    and AHBot auctions are untouched.
  - A failure injected during dbimport rolled back to the previous world
    (319 tables), configs and module set.
- Restoring a world database from a dump made by CI could replace the
  dbimport rebuild later, if it is faster. It would need a new artifact.

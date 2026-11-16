# 0019. The picked folder is the data root; renamed or moved data roots

- Status: Accepted
- Date: 2026-11-15

## Context

ADR 0017 let the player pick the install location, but used the
`RealmKeeperData` subfolder of the picked folder, so that an uninstall,
which deletes the launcher's folders in the data root (`components`,
`serverdata`, `assets`, `store`), could never touch the player's own files.
Beta feedback:

- Players expect the folder they pick to be the one used. The extra
  subfolder is surprising.
- A data root that was renamed or moved could not be loaded again. On start,
  `location.json` still named the old place. Rendered configs hold the
  instance's absolute paths (`DataDir`, `LogsDir`, `SourceDirectory`,
  `MySQLExecutable`), and so do the server data overlay junctions (ADR 0013).
  Local check (2026-11-15): after renaming the data root, MySQL starts from
  the new place (`my.ini` is rewritten at each start), but the configs still
  name the old one.

## Decision

1. **The picked folder itself is the data root.** It must be new or empty,
   or already a data root. A data root has the marker file
   `.realmkeeper`, which the launcher writes whenever it opens a root,
   or the `instances` and `state` folders an earlier launcher made.
   Otherwise the error is `location.notEmpty`. The safety of ADR 0017 is
   kept: an uninstall still removes only the launcher's own folder names.
2. **A missing data root is never recreated.** When `location.json` names a
   folder that no longer exists, the launcher opens on its own folder and
   reports `missing`. The UI asks the player to locate the folder or use
   the default.
3. **Pointing at another data root is allowed at any time** (servers stopped,
   no operation running), even while realms are installed. Changing to a new
   empty folder still requires no realm to be installed, since nothing is
   moved.
4. **Moved realms:** an installed instance whose rendered configs contain
   none of its current absolute paths is `moved`. `Launcher::server` refuses
   to start it (`instance.moved`). Resume (`rk resume`, or "Update the
   paths" on Home) forgets the `modules-<id>` journal and runs the module
   operation again. That rewrites the configs, overlay and module files, and
   runs dbimport (about 30 s, verified on `pubtest` renamed and renamed back,
   2026-11-15).

## Consequences

- Roots made by earlier launchers (`<picked>\RealmKeeperData`) keep working,
  and get the marker the next time they are opened.
- A folder with the player's own files cannot become the data root. They
  pick an empty folder or create one in the dialog.
- `moved` detection reads the instance's top-level config files at each
  overview (a few small files).
- Switching between data roots while realms are installed leaves the other
  root untouched; its realms come back when the player points at it again.

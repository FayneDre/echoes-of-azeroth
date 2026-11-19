# 0018. Beta readiness: disk space, diagnostics, live state, daily backups, tray

- Status: Accepted
- Date: 2026-11-15

## Context

The closed beta puts the launcher on computers we do not control. The
problems we expect most often are not bugs but environments:

- **Disk space.** The installer said "about 2 GB to download", but one
  installed realm takes about 9 GB. Measured on a local install
  (2026-11-15): MySQL 8.4.13 unpacked 1 263 141 970 B (download
  281 154 912 B), server data v20.0 unpacked 3 230 904 757 B (download
  1 196 168 257 B), one AzerothCore realm (server files, SQL tree,
  databases) 2 856 909 827 B. The downloads stay in the store cache. A full
  disk failed halfway with a raw I/O error.
- **Taken ports.** A second server on 3724 / 8085, or a server left over
  from a crash, was reported as "port in use" without saying which program
  held it. A taken database port was reported as a generic database error.
- **Support.** Bug reports need the launcher version, the realms and their
  state, the game folder check and the end of the server logs. Players
  cannot be expected to find `instances/<id>/logs`.
- **Comfort.** Players had to browse for the game folder, could not see
  whether bots were logged in, saw the launcher over the game, had no
  backup unless they made one, lost the window position at each start, and
  got a window taller than a 1366 x 768 laptop screen.

## Decision

1. **Free space checked before the work** (`rk_store::space`, `fs4`):
   a download checks the bytes it still has to write (the partial file is
   preallocated), an unpacking checks the sum of the archive's entries, with
   a 256 MB margin. Before a new realm is installed, the launcher sums what
   is missing: downloads not in the store, the catalog's new optional
   `unpackedSize` (component platform build, client `serverData`) and the
   emulator's `instance.diskSize`; installed shared parts count for nothing.
   The install wizard shows needed and free space and refuses a disk known
   to be too small. Errors: `disk.full` (needed / free figures) and
   `disk.fullNow` (an I/O error saying the disk is full), found anywhere in
   the error chain.
2. **Taken ports name their program:** `port.inUseBy` with the program
   found by `netstat -ano` (a listening socket has a `:0` remote end; the
   state column is translated, so it is not read) then `tasklist`. The
   database port uses the same codes.
3. **Diagnostic report** (`rk_launcher::diagnostic`, `rk diagnostic`,
   "Copy the diagnostic" in the footer): versions, free space, game clients
   and their check, every realm (record without secrets, operation
   journals, backups) and the last 25 lines of its six most recent logs,
   redacted like crash reports. Built on demand, copied by the player,
   never sent.
4. **Game folder found automatically** (`rk_client::find`, `rk client
   find`): drive roots C to Z and the player's own folders, every folder
   one level down, deeper only into folders whose name suggests a game
   (`wow`, `warcraft`, `game`, `jeu`, `client`...), at most four levels and
   6 000 folders. Only folders `detect` accepts are offered.
5. **Live realm state:** `EmulatorDriver::population` (default: unknown);
   the session counts every 20 s and sends `Population { players, bots }`.
   AzerothCore: characters with `online <> 0`, bots being those of
   `RNDBOT` accounts (Playerbots' `RandomBotAccountPrefix` default
   `rndbot`, upper-cased like every account name, verified in
   `conf/playerbots.conf.dist` at 405f811). Uptime counts from the
   session's `ready` event.
6. **Daily backups:** `daily-<secs>.sql` (player databases, like a manual
   backup), at most one a day, made in the background after a session ends
   cleanly; the newest 7 are kept, separately from the `before-*` dumps (5).
   The next start or change of that realm waits for it; quitting the
   launcher lets it finish. Not made by the CLI.
7. **Launcher preferences** in `<app dir>/preferences.json` (not a realm's
   settings): `dailyBackup` and `hideWhilePlaying`, both on by default; a
   missing or unreadable file gives the defaults.
8. **Notification area icon** (Tauri `tray-icon`): show the launcher, stop
   the servers, quit (servers stopped cleanly first, as when closing the
   window). Its texts come from the UI's language. With `hideWhilePlaying`,
   the window hides when the game starts from the launcher (solo or LAN)
   and comes back when it exits.
9. **Window:** `tauri-plugin-window-state` restores position, size and
   maximised state (first start centred). At start, a window or minimum
   size larger than the screen's work area is reduced to fit it.
10. **Release notes after a launcher update:** the notes of the update
    feed are kept in local storage before the installer restarts the
    launcher and shown once that version runs.
    *Amended 2026-11-18 (0.3.2):* the notes live in
    `app/src/lib/release-notes.json` (one entry per version, one line per
    change, English and French; the release workflow fails without them).
    `tools/ci/app-release.mjs` writes them to the feed (`notesByLocale`,
    plus an English `notes` text for 0.3.x launchers) and to the GitHub
    release. An available update is a title bar button opening a dialog
    with its notes, the install button and the download progress; after
    the restart, the new build shows its own bundled notes once (last
    version run kept in local storage).

## Consequences

- Catalog manifests gain three optional measured sizes; they must be
  measured again when MySQL, the server data or the realm content changes a
  lot (the free-space checks of downloads and unpacking stay exact).
- The port owner lookup spawns two Windows tools only when a start already
  failed; other platforms report the port without a program.
- The population query runs every 20 s while the servers run (one `mysql`
  call). Emulators whose driver cannot tell show no figures.
- A daily backup starts the database again for about 15 s after a play
  session; a start in that window waits for it.
- The tray icon keeps a hidden window reachable; closing the window still
  quits the launcher.
- Follow-up: measure `diskSize` again after a large module or content
  change; CLI daily backups if power users ask for them.

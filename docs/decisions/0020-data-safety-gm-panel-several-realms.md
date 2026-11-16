# 0020. Data safety, GM panel and add-ons, several realms

- Status: Accepted
- Date: 2026-11-16

## Context

Before the closed beta, three groups of gaps came out of the code and of
the maintainer's use of `pubtest`:

- **Player data at risk outside the launcher's own failures.**
  - Windows shutting down or logging off killed worldserver and MySQL
    without a world save.
  - A data root inside a synchronised folder (OneDrive, Dropbox, Google
    Drive) gets a MySQL data directory uploaded while the server writes
    it. Windows often redirects "Documents" to OneDrive.
  - The server data overlay (ADR 0013) needs NTFS directory junctions and
    hard links. FAT32/exFAT drives and most network shares fail halfway
    through an install.
  - An antivirus that quarantines `worldserver.exe` or `legacy.dll` showed
    up as a start failure with no cause given.
  - A computer going to sleep cut long downloads.
- **Comfort while playing.**
  - GM commands had to be typed in the console.
  - worldserver takes 1-4 min to start with no sign of progress.
  - Playerbots is far easier to drive with its client add-on (MultiBot),
    which players had to find and install by hand.
- **One realm per game version.** A player could not try other modules
  without risking the main game, nor move a realm to another computer.

Facts checked locally (2026-11-16, `pubtest`, build b8a8e89ebf07aeb9):

- Copying a cleanly stopped realm takes 32 s. Without `auto.cnf`,
  `#innodb_temp/` and spare redo files, 1.3 GB of the database is copied;
  `bin/` and `sql/` are hard-linked. The copy ran dbimport and started:
  worldserver was ready in 79 s with its 500 bots, then stopped cleanly.
- Exporting the realm took 30 s. The player databases dump to 72 MB of
  SQL, 21 MB in the deflated ZIP.

## Decision

### Data safety

1. **Clean stop at Windows shutdown or log-off.**
   - The `Exit` run event stops the running servers cleanly before the
     process ends.
   - While servers run or an operation works, the window registers a
     shutdown block reason (`ShutdownBlockReasonCreate`). Windows then
     shows why it waits instead of killing the launcher at once.
2. **Awake while working.** An operation holds a system-required power
   request (`PowerCreateRequest`) for its whole length.
3. **Install location checks** (`location::check`, before a first install
   and when a folder is picked):
   - refused inside a known synchronised folder (`location.synced`, the
     client is named);
   - refused where a directory junction cannot be made
     (`location.unsupported`).
4. **Quarantine detection.**
   - Each extraction lists the `.exe` and `.dll` it wrote, with their
     sizes, in `.rk-binaries`.
   - `Launcher::server` checks the server folder and every component
     against that list before a start. A missing or changed file is
     `server.filesMissing`, and the message offers Repair and explains
     antivirus exclusions.
   - Windows errors 225/226 (virus infected / deleted) map to
     `security.blocked`.

### Comfort

5. **GM panel.**
   - The emulator manifest declares `consoleActions`: an id, a label and
     a command with `{param}` placeholders. Each param has a kind:
     character, number (min/max/default), text (max length), choice or
     location.
   - `rk_core::console::render` checks every value before it builds the
     line. Names are letters only, numbers stay in range, and text is one
     line with no quote, so a value can never add a second command.
   - The driver lists the characters in the world (players first, then
     bots) and the named teleport places.
   - The panel is the "Game master" part of the Account tab, next to
     account creation; that tab also sets an account's GM level.
   - Home has a realm strip: the realm shown (named, the first one "My
     realm"), rename, duplicate, export, import and uninstall.
6. **Start progress.**
   - A process may declare `startup` markers: an output line, a fraction
     and a localized label, in order. The session reports the furthest
     marker reached; lines that never come are skipped.
   - When the launcher is hidden (tray, minimised), a Windows
     notification says the realm is ready.
7. **Client add-ons as module assets** (extends ADR 0013).
   - An asset source can be an upstream repository at a pinned commit:
     `repository` + `commit` + `size` + `sha256`, downloaded as
     `{repository}/archive/{commit}.zip` and never re-hosted.
   - The `addons` entries map archive folders to folders under the
     client's `addons` directory (`Interface/AddOns`). They are tracked
     and removed like the other managed client files.
   - First use: the Playerbots option "MultiBot add-on" (off by default).

### Several realms

8. **Realm names.** `instance.json` gets an optional `name` (one line, at
   most 48 characters). The UI shows it instead of the id. The realm shown
   for each game version is a per-viewer choice; the oldest realm is shown
   by default.
9. **Duplicate** (`rk realm duplicate`, Home "Duplicate"). This is a cold
   copy of a stopped, fully installed realm:
   - the source lock is held, and a MySQL still running is stopped first;
   - `bin/` and `sql/` are hard-linked, since an update or repair
     replaces them whole (ADR 0004 atomic swap) and never edits them in
     place;
   - everything else is copied except `logs/`, the `data/` overlay,
     locks, unfinished-change files, `auto.cnf`, `*.pid`, `mysqld.log`,
     `#innodb_temp/` and spare redo files;
   - the copy is built in `instances/.copy-<id>-<secs>` and renamed into
     place;
   - it gets its own database port (`my.ini` and `client.cnf`, and every
     MySQL tool call passes `--port`), then the module operation writes
     its configs, overlay and module files.
   If the copy fails, its folder is removed. Game ports stay shared: only
   one realm runs at a time.
10. **Export** (`rk realm export`, Home "Export"). An `.rkrealm` file is
    a ZIP that holds:
    - `realm.json`: format 1, emulator, flavor, modules, options, name,
      network, build, export time and launcher version. Nothing in it is
      tied to the computer (no ports, paths or passwords);
    - `bindings.json`: the accounts and characters that module settings
      name;
    - `player-data.sql`: the player databases only. Content databases
      are rebuilt by the build.
11. **Import** (`rk realm import`, Home "Import a realm"):
    - The file is read first. Its emulator, flavor and modules must exist
      in this catalog (`realm.exportUnsupported`).
    - The dump becomes the backup `import-<secs>.sql` of the new realm,
      kept like a manual one.
    - The realm is installed from the signed index. Then its options and
      name are set, the player databases are restored (ADR 0016, then
      dbimport migrates them to the current build), the exported bindings
      replace the ones the install made, and the module operation writes
      the configs.
    - `import.json` in the instance folder records what is left. The
      overview reports `unfinished: import-<id>`, and `resume` finishes
      the import.

### Player help and checks

12. **Help from every error.** `docs/faq.md` and `docs/faq.fr.md` have one
    section per kind of problem, with stable anchors. `ErrorBox` maps the
    error code to its section and opens it in the player's language.
13. **Accessibility.**
    - ~~A text size preference (normal, large, larger) scales the root
      font.~~ Removed on 2026-11-16 at the maintainer's request.
    - `prefers-reduced-motion` turns transitions off.
    - Dialogs close with Escape and focus their first field. Since
      2026-11-16 every dialog is `Modal.svelte`, and anything the player
      unfolds to read (technical details, crash report, LAN join) opens in
      one instead of growing the page.
14. **UI smoke test in CI.** `tools/ci/ui-smoke.mjs` builds a debug app
    whose WebView2 debugging port is open (a `--config` override), then
    drives it through `tools/ci/cdp.mjs` (Node's built-in WebSocket, no
    dependency) on an empty data root. It checks the shell, both languages,
    the install wizard, the disabled tabs, the absence of a horizontal
    scrollbar, a name on every button and the absence of script errors.
    Screenshots are kept as a CI artifact.

## Consequences

- A shutdown with servers running now waits for the clean stop: up to a
  minute with 500 bots. Windows shows the reason meanwhile.
- Players who keep their files under OneDrive must pick another folder.
  The message says why.
- A duplicate costs the size of its database on disk (about 1-2 GB), and
  the server files nothing. A hard-linked file edited in place would show
  in both realms; nothing in the launcher does that.
- An export holds account names and password verifiers (SRP6, not the
  passwords). The player shares it like a save file.
- An import needs the network (server build, server data) like an install.
  Restoring data from an older build relies on dbimport, as an update does.
- Add-ons come only from repositories whose archives are reachable at a
  pinned commit, like the other assets.
- New error codes: `realm.exists`, `realm.invalidName`,
  `realm.exportInvalid`, `realm.exportUnsupported`, `realm.otherGame`
  (UI), `location.synced`, `location.unsupported`, `server.filesMissing`,
  `security.blocked`.

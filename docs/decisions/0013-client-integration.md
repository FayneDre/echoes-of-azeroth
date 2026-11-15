# 0013. Client integration: managed files, module assets from upstream, server data overlay

- Status: Accepted
- Date: 2026-11-15

## Context

Phase 4 makes the launcher prepare and start the player's own client
(principle 7: never distributed). Facts, verified on 2026-11-15:

- `Wow.exe` 3.3.5a carries a `VS_FIXEDFILEINFO` with file version
  3.3.5.12340. The client reads `Data/<locale>/realmlist.wtf`, and it also
  saves `SET realmList "<address>"` in `WTF/Config.wtf`. Its `Cache/WDB`
  holds what the last server sent (items, creatures).
- Individual Progression ships its optional files as archives in its own
  repository at the pinned commit (`optional/patch-V.7z`, `patch-S.7z`,
  `dbc.7z`; module README and `optional/patch-explanations.txt`):
  - patch-V and patch-S each pair a client MPQ (`patch-V.mpq` /
    `patch-S.mpq`) with a server `Spell.dbc`, and they are mutually
    exclusive;
  - `dbc.7z` holds the visual MPQs `Patch-J.mpq` and `Patch-U.mpq`, plus four
    server DBCs.
- Server data (`DataDir`) is shared by every instance of a client version
  (ADR 0006). worldserver reads `dbc/`, `maps/`, `vmaps/`, `mmaps/` and
  `Cameras/` under it, about 2.5 GB, and has no key for individual
  directories.
- Module options were declared in the catalog but only their defaults were
  applied, and the asset `artifact` field referred to CI artifacts that were
  never built.

## Decision

1. **Option values in the instance record.** `instance.json` gains `options`
   (module → option → value). A value is validated against the manifest when
   it is set (`eoa option set`). A value that no longer fits the option falls
   back to its default. Chosen values are rendered as `User` config layers.
2. **Module assets come from upstream.** An asset names an archive of the
   module repository (`source.path`, `size`, `sha256`) and the members to use
   (`client` / `server`, each `{from, to}`).
   - The launcher downloads it from the pinned commit (GitHub raw URL) into
     the store, verifies it, and unpacks it once into
     `assets/<archive sha256>/`. Both `.zip` and `.7z` are supported, the
     latter in pure Rust.
   - The project never re-hosts these files, as with server data in
     ADR 0006.
   - The `modules-<id>` operation downloads and unpacks the assets selected
     by the option values.
3. **Per-instance server data overlay.** When a selected asset has server
   files, `DataDir` points to `instances/<id>/data/`.
   - Each top-level directory without an override is a link to the shared
     data. On Windows these are junctions, which need no privilege.
   - A directory with overrides is rebuilt with hard links to the shared
     files, falling back to copies; overridden files are left out and
     copied from the asset.
   - The overlay is built in a staging directory and swapped in with
     `DirSwap`. It is undone on rollback, and removed once no override is
     left. Links are removed, never followed.
4. **Registered client and managed files** (`eoa-client`, state in
   `clients/<client id>.json`).
   - **Registration:** `eoa client set` checks the executable's build and
     the locale folders.
   - **Preparation:** before each launch the launcher:
     - rewrites `realmlist.wtf`, and the `saved` setting declared in the
       client manifest (`SET realmList` in `Config.wtf`, only when the file
       exists);
     - clears the cache when another instance was played last;
     - reconciles the patch files with the instance's choices.
   - **Tracking:** managed files are tracked by sha256.
     - A file of the player at the same path is moved to
       `<name>.eoa-backup` and put back when the launcher removes its own.
     - The same content already in place is adopted.
     - A tracked file the player changed is never deleted.
     - Each change is recorded before it is made, so an interrupted
       preparation is finished by the next one.
   - **Locking:** a client lock (`clients/<id>.lock`) is held while the
     client is prepared and while the game started by the launcher runs.
5. **Synchronised launch (`eoa play`).** The launcher prepares the client
   first, to fail fast. It then starts the servers, and launches the
   executable from the client folder when the last server is ready. The
   local client always uses `127.0.0.1`.
   - By default the servers stop when the game exits (they use several GB
     of RAM); `--keep-running` keeps them up.
   - The game is not tied to the launcher: closing the launcher never
     closes the game.

## Consequences

- A module that ships client or server files needs no CI work: its manifest
  pins the archive and its members (`add-module` skill). Only GitHub
  repositories are supported for now.
- Overlays add no copy of the large directories. The rebuilt `dbc/` is
  87 MB of hard links, which takes about 1 s.
- Hard links share their content with the shared data. This is acceptable
  because the servers never write their data files, and overridden files are
  never linked.
- The client is changed only through tracked files and the two address
  lines. Its other settings are untouched.
- `realmName` in `Config.wtf` is left alone. Every instance names its realm
  the same way, so the saved name keeps matching. Assumed, not verified: a
  name that does not exist only makes the client show the realm list.
- Clients on other computers (LAN) still need their address set by hand.
  The launcher shows it; a "prepare a LAN client" helper is future work.

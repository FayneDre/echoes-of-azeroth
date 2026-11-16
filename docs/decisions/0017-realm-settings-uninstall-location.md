# 0017. Realm settings, uninstall and the install location

- Status: Accepted
- Date: 2026-11-15

## Context

- **Realm settings.** "Server settings" listed only the modules. Players
  expect the usual realm settings: rates, realm type (PvP, PvE, RP),
  starting level and money, interaction between factions. All of them are
  `worldserver.conf` keys of the core. Verified in the fork's
  `worldserver.conf.dist` at the pinned commit `32fd347`:
  - `Rate.XP.Kill`, `Rate.XP.Quest`, `Rate.XP.Explore`;
  - `Rate.Reputation.Gain`, `Rate.Honor`, `Rate.Drop.Money`;
  - `Rate.Drop.Item.Uncommon`, `Rate.Drop.Item.Rare`, `Rate.Drop.Item.Epic`;
  - `GameType` (0, 1, 4, 6, 8, 16), `AllowTwoSide.Interaction.*`;
  - `Instance.IgnoreLevel`, `Instance.IgnoreRaid`;
  - `MaxPlayerLevel`, `StartPlayerLevel`, `StartHeroicPlayerLevel`,
    `StartPlayerMoney`;
  - `AllFlightPaths`, `AlwaysMaxSkillForLevel`, `SkipCinematics`.

  Rates are decimal values.
- **Realm type in the realm list.** The client shows the realm type from
  the authserver, which sends `realmlist.icon` from the auth database
  (`tinyint`, base SQL of the fork). worldserver reads `GameType` but does
  not update that column.
- **Uninstall.** There was no way to remove a realm. Its folder holds the
  player data (`db/`). The game folder may hold files the launcher put
  there for that realm (ADR 0013). The shared downloads are about 6.5 GB:
  MySQL, server data, module assets and the download cache.
- **Install location.** Everything went under `%LOCALAPPDATA%` (principle
  6), so always on the system drive. A realm needs about 7 GB.

## Decision

### Realm settings

- The emulator manifest gets `settings`: sections (`id`, localized
  `label`) of options.
  - These are the same option types as module options: `boolean`,
    `integer`, `choice` with config changes, and a new `number` type for
    decimal values.
  - Targets are core config files only (`instance.config` files). A key the
    launcher imposes is refused. Choices cannot install assets.
  - `rk-catalog` validation checks all of this.
- The values live in `instance.json` with the module options, under the
  reserved key `realm` (`REALM_OPTIONS`; no module may use that id). They
  are set like module options:
  - `set_options(id, "realm", ...)`;
  - `rk option set <id> realm <option>=<value>`.
- Only values the player changed are written, as a `User` layer. Untouched
  keys keep the upstream default, and the layer sits below the modules'
  `CoreOverride` values, which stay locked.
- A value that no longer fits its option (out of range, removed choice)
  falls back to upstream. A whole number is a valid decimal value. Decimal
  input accepts `2,5`.
- The AzerothCore driver reads `GameType` from the rendered
  `worldserver.conf` before each start and writes it to `realmlist.icon`
  together with the address. The `DriverContext` gets the instance's
  `configs` folder.
- UI: "Server settings" shows one page per section (Rates, Realm,
  Characters) plus Modules. Rates default to the upstream value 1; nothing
  is throttled (principle 2).

### Uninstall

- `Launcher::uninstall(id, keep_backup)` requires the servers to be stopped
  (instance lock). The steps are:
  1. With `keep_backup`, dump the player data first (a manual backup, never
     pruned). Installing the realm again lists it for a restore
     (ADR 0016).
  2. Stop the database if it still runs.
  3. Remove the files the launcher put in the game folder for this realm.
  4. Rename `instances/<id>` to `.removed-<id>-<secs>`, forget its journals,
     then delete the folder. Leftovers of an interrupted delete are removed
     next time.
- When no realm is left, the shared downloads go too: `components`,
  `serverdata`, `assets` and `store`, with their journals. Kept:
  - backups;
  - the registered clients;
  - the accepted index (anti-rollback).
- Exposed as `rk uninstall <id> [--no-backup]`, and as a confirmation
  dialog in Home › Maintenance. "Keep a backup" is checked by default.

### Install location

- `%LOCALAPPDATA%\RealmKeeperData` stays the launcher's own folder, holding
  the crash reports and `location.json`. `location.json` points to the data
  root, which holds the realms, downloads, backups and journals.
- The player may pick the data root in the install wizard, only while no
  realm is installed (nothing is ever moved). The data root is the
  `RealmKeeperData` subfolder of the picked folder, never the picked folder
  itself, since an uninstall deletes the launcher's folders in the data
  root. The folder is checked writable.
- `RK_ROOT` still overrides everything (development, tests).

### Smaller changes in the same pass

- A running session accepts a game launch, so the Home panel shows Play
  while the servers are online. A game launched that way leaves the servers
  running.
- Console output is decoded as UTF-8, else as code page 850. AzerothCore
  converts its console output to the OEM code page (`utf8printf`), so the
  banners showed as replacement characters.

## Consequences

- New realm settings are catalog data, verified against the pinned
  `.conf.dist` like module options. The `bump-pins` skill must re-check
  their keys.
- Principle 6 now reads: everything under one data root, `%LOCALAPPDATA%`
  by default, still without admin rights. Moving an installed realm to
  another drive is not supported yet (uninstall with a backup, change the
  location, install, restore).
- `OptionValue` and `InstanceRecord` are no longer `Eq` (decimal values).
- Code page 850 is an approximation for systems whose OEM code page is
  not 437 or 850. Box drawing, ASCII and Western European letters cover
  AzerothCore's output.
- The realm name in the realm list stays "AzerothCore". It is a database
  value, not a config key: a later setting could go through the driver like
  the icon.

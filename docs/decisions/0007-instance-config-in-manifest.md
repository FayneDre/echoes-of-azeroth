# 0007. Instance configuration in the emulator manifest; instance layout

- Status: Accepted
- Date: 2026-11-14

## Context

Installing an instance means writing the server configs with values only the
launcher knows: paths (data, logs, SQL source, `mysql` client), ports, the bind
address, database credentials, which module SQL dbimport may apply. The key
names are emulator-specific (AzerothCore: `DataDir`, `LogsDir`,
`SourceDirectory`, `MySQLExecutable`, `RealmServerPort`, `WorldServerPort`,
`BindIP`, `Updates.EnableDatabases`, `Updates.AllowedModules`, and connection
strings `host;port;user;password;database`; checked in the fork's
`worldserver.conf.dist`, `authserver.conf.dist` and `dbimport.conf.dist` at
commit `32fd347`). `authserver.conf.dist` defaults to
`Updates.EnableDatabases = 1`: left alone, authserver would migrate the auth
database itself, outside the launcher's backups.

ADR 0004 puts everything describable in data and only the rest in drivers.
Database connection keys are already declared in the manifest.

The server artifact (CI) contains `dist/` (executables, DLLs,
`configs/*.conf.dist`, `configs/modules/*.conf.dist`) and `sql/`. Modules are all
compiled in (ADR 0003), so every module reads its config file even when the
player did not choose it.

## Decision

- The emulator manifest gets an `instance` section:
  - `connection`: template of a database connection value
    (`127.0.0.1;${db.port};${db.user};${db.password};${db.name}`);
  - `config`: file → key → value the launcher imposes (Launcher layer, locked),
    with placeholders from a closed list: `${dir.instance}`, `${dir.logs}`,
    `${dir.sql}`, `${dir.data}`, `${tool.mysql}`, `${port.<name>}` (names of
    `ports`), `${network.bind}`, `${modules.dbimport}`. A value that is exactly
    one numeric placeholder (ports) is written as a number;
  - `dbImport`: the tool that applies SQL (`executable`, `configFile`).
  Unknown placeholders are catalog validation errors.
- Every server-side migration switch is off in the core files
  (`Updates.EnableDatabases = 0` in worldserver and authserver); only the
  launcher's dbimport run migrates, after a backup.
- Config layers per file: option defaults and the `enable` values of chosen
  modules (ModuleDefault), player values (User), module core overrides and the
  `disable` values of modules not chosen (CoreOverride, locked), `instance`
  values and connections (Launcher, locked). Every catalog module compatible
  with the flavor gets its config file, enabled or disabled.
- Layout under the data root (`%LOCALAPPDATA%\EchoesOfAzeroth`):
  ```
  components/<id>/                 shared runtimes (own operation each)
  serverdata/<client>-<version>/   shared server data (own operation each)
  instances/<id>/                  cwd of the servers
    bin/       artifact dist/      sql/      artifact sql/
    configs/   rendered            logs/
    db/        eoa-db instance     instance.json, secrets.json
  state/                           journals and locks
  ```
- `eoa install` runs the shared operations first, then one `instance-<id>`
  operation: extract `bin` and `sql`, initialise and start MySQL, create the
  databases and the service account, render the configs, run dbimport, stop
  MySQL cleanly. Its rollback never touches shared directories.

## Consequences

- A new emulator describes its instance config as data; the driver keeps only
  what cannot be declared (accounts, console, realm address).
- Server data is shared through an absolute `DataDir`, not junctions; module
  overlays of data files (e.g. `Spell.dbc`) will need a per-instance directory
  later.
- Ports and the MySQL port are chosen at instance creation and persisted in
  `instance.json`; secrets (service account password) live in `secrets.json`,
  never in journals.
- `schemaVersion` stays 1: the catalog is not published yet.

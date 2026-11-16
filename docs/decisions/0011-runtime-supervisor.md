# 0011. Runtime supervisor: declared stop methods, accounts written by the driver

- Status: Accepted
- Date: 2026-11-15

## Context

Phase 3 runs installed instances: MySQL, then authserver and worldserver, each
started only once the previous one is ready, and stopped cleanly. The facts
below were checked in the Playerbot fork at the pinned commit `32fd347`:

- When stdin is a pipe, worldserver reads console commands with
  `std::getline` and calls `World::StopNow(SHUTDOWN_EXIT_CODE)` at end of input
  (`CliRunnable.cpp`), which is a clean shutdown: players are saved. Observed
  locally on 2026-11-14.
- authserver has no console. Everything it does is written to the database
  right away, so killing it loses nothing.
- The launcher may die (crash, kill, power loss). Servers must neither be
  left without a parent forever nor be killed before they save.
- worldserver needs MySQL to save during its shutdown.
- Accounts: `AccountMgr::CreateAccount` upper-cases the name and the password,
  stores a 32-byte salt and an SRP6 verifier
  `v = g^H(s | H(USER:PASS)) mod N` (SHA-1, g = 7, 256-bit N, little-endian),
  inserts `realmcharacters` rows and, for GM levels, `account_access`
  (`SRP6.cpp`, `AccountMgr.cpp`, `LoginDatabase.cpp`). Limits: 17-character
  names and 16-character passwords.
- `realmlist` row 1 (`RealmID = 1` in `worldserver.conf.dist`) holds the
  address and port that authserver sends to the client.

## Decision

- **Stop methods are data.** Every manifest process declares `stop`:
  - `close-stdin` (+ `timeoutSeconds`, default 300): stdin is closed and
    the supervisor waits for the process to exit, then kills it after the
    timeout. When the launcher dies, its end of the pipe closes, so the
    process still shuts down cleanly. worldserver uses it, with 600 s.
  - `terminate`: the process is killed. Only for processes with no unsaved
    state. Such a process is put in a kill-on-close **Job Object**, so it
    dies with the launcher. authserver uses it.
  - Processes may also declare `args`. The working directory is always the
    instance root.
- **Console.** A `close-stdin` process takes console commands: its stdin is
  a command line. No other marker is needed.
- **MySQL is never killed.** If the launcher dies, MySQL keeps running, so
  a world shutdown can still save. The next start or stop adopts it: `rk-db`
  pings it with the instance's root credentials.
- **Readiness.** A TCP readiness port that is already open before spawning
  is an error (`PortInUse`). Otherwise an orphan or a foreign program would
  pass the check. The startup budget is 20 min per process, and an early
  exit fails at once with the last 60 output lines.
- **Output.** Output goes to `logs/<process>.console.log`; the previous run's
  file is kept as `.previous.log`. It is also sent to a broadcast channel for
  the UI and the CLI.
- **Driver interface.** The `EmulatorDriver` trait lives in `rk-runtime`:
  `prepare_start` (runs once the database is up, before the servers) and
  `create_account`. The AzerothCore driver (`drivers/azerothcore`) does both
  directly in the database:
  - It recomputes the SRP6 registration exactly like upstream, so creating
    an account needs no running worldserver.
  - Account names are ASCII letters and digits, and passwords printable
    ASCII. This keeps every value that reaches SQL safe.
  - Before each start, it points realm 1 at the current address
    (`127.0.0.1`, or the LAN address that this computer uses for outgoing
    traffic) and at the world port.

## Consequences

- If the launcher dies, worldserver saves and exits, authserver dies, and
  MySQL lingers until the launcher runs again. A dedicated watchdog could
  stop MySQL later if this matters in practice.
- Emulators with other stop conventions add a new `stop` type (schema +
  `StopMethod` + supervisor). That is a small, reviewed change.
- The SRP6 code must follow upstream: an account-creation test against a
  real server stays part of the phase 3 checks.
- The driver registry is a `match` in each binary (CLI now, Tauri later)
  until a second driver exists.

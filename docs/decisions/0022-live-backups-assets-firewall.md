# 0022. Backups while the servers run, asset pruning, LAN firewall rule

- Status: Accepted
- Date: 2026-11-18

## Context

ADR 0016 left four things open: backups while the servers run, pruning
`assets/`, a firewall rule for LAN hosting (principle 6), and smoke tests
that go further than "the servers start".

- AzerothCore keeps players in memory and saves them every
  `PlayerSaveInterval` (15 min by default). Its console command `saveall`
  (`ObjectAccessor::SaveAllPlayers`, `Console::Yes`) queues a save of every
  player and prints `All players saved.` (`LANG_PLAYERS_SAVED`, id 15)
  (fork `cs_misc.cpp`, `ObjectAccessor.cpp`, `acore_string` @ 32fd347).
- `mysqldump --single-transaction` (already used by `rk-db`) reads InnoDB
  tables in one consistent snapshot without blocking the server.
- Module assets are unpacked once into `assets/<archive sha256>/`. The
  server data overlay **copies** the override files and links only
  directories of the shared server data; client files are copied into the
  game folder before a launch. Nothing links into `assets/`.
- A Windows firewall rule needs administrator rights. `New-NetFirewallRule`
  takes the program, the protocol, the local port and the profiles; reading
  rules (`Get-NetFirewallRule`) needs no elevation. A network that Windows
  treats as **Public** does not get rules made for Private networks
  (local test, 2026-11-18: this PC's Wi-Fi is Public).
- The authserver logon is SRP6 (challenge, proof, realm list) as in the
  fork's `AuthSession.cpp` / `SRP6.cpp`; a client can be written in a few
  hundred lines of Node (BigInt, SHA-1).

## Decision

1. **Backups while the servers run.** The running *session* does it, as
   the holder of the instance lock: `SessionControl::while_running(work)`
   saves the players, then runs `work` (`Launcher::backup_live`: the
   running database dumped in one transaction, left running) inside the
   supervise loop, next to crash detection and console commands; a stop
   or the game's exit requested meanwhile waits for it. Refused (`None`,
   `backup.notReady`) while the servers start or stop: worldserver may
   still be migrating the Playerbots database. `backup_now` (servers
   stopped, the lock) is unchanged; the CLI only backs up a stopped realm.
   A restore still needs the servers stopped. Saving: the emulator
   manifest declares `savePlayers { command, done, timeoutSeconds,
   settleSeconds }` (AzerothCore: `saveall`, `All players saved.`, 60 s,
   then 3 s for the queued writes); `Running::save_players` sends the
   command and waits for the line (without it, the backup holds the last
   save). Deleting a backup works while the lock is held by something that
   does not read backups, never while a change is in flight
   (`pending.json` or an unfinished journal: a restore reads its backup).
2. **Asset pruning.** After a module or option change, an update, repair,
   restore or uninstall, `Launcher::prune_assets` removes the
   `assets/<sha256>/` folders that no realm selects (resolution + option
   values of every record). Nothing is removed while a realm is not fully
   installed, has an unfinished operation or a `pending.json`. The archives
   stay in the store: turning an option on again unpacks without a
   download. Only 64-hex names are touched (staging folders are not).
3. **LAN firewall rule** (`rk-launcher::firewall`, Windows only). One
   inbound rule per server program of the realm: TCP, that program only,
   limited to the port its manifest `readiness` waits on, profiles Private
   and Domain, group `RealmKeeper`, display name
   `RealmKeeper <realm> <process> <hash of the program path and port>` (a
   moved realm or a changed port shows as not allowed until the rule is
   made again). Made or removed by one elevated PowerShell
   (`Start-Process -Verb RunAs`, the script as `-EncodedCommand`: no path
   is quoted on a command line); only ERROR_CANCELLED (1223) in the
   exception chain is `firewall.declined`, any other failure is
   `firewall.failed` with its reason in `state/firewall-error.txt` (a full
   path: the elevated process starts elsewhere). Uninstalling a realm
   removes its rules when it has some (one prompt, a declined one leaves a
   warning).
   The status is read without elevation, with whether Windows treats the
   LAN interface's network as Public. UI: a row of the Maintenance column
   for LAN realms; CLI: `rk firewall show|allow|remove <id>`. Never done
   automatically.
4. **Smoke test, round B (modules on)**: a test account and the AHBot owner
   (account + character, `AuctionHouseBot.Account/GUID`) are written as the
   driver does; once the world is ready, `tools/ci/auth-client.mjs` logs in
   on authserver (SRP6 challenge, proof checked both ways, realm list: the
   world port, realm online), then the test waits (up to `--live-min`, 10)
   for random bots online and AHBot auctions in `auctionhouse`. The client
   checks its SRP6 against the verifier made by worldserver (srp6.rs) and a
   simulated server at every run.

## Consequences

- A live backup misses at most the writes after `saveall` was confirmed
  plus 3 s. Only the app backs up a running realm (its session holds the
  servers); `rk backup create` on a realm started by `rk start` in another
  process is busy, as before.
- The firewall rule only helps on Private networks: the UI says so when
  Windows treats the network as Public, instead of widening the rule to
  Public networks. Uninstalling an allowed realm asks for elevation once
  more, to remove its rules.
- World login (character on worldserver) is not covered by the smoke test:
  it needs the world protocol's header encryption.
- Not done yet: a size budget for backups, pruning the store's old blobs.

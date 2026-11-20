# 0025. A party of bots formed from the launcher

- Status: Accepted
- Date: 2026-11-20

## Context

Players asked to form a party of Playerbots from the launcher: pick the
character they play, then add bots by role, class and spec, at the
character's level or another one. Typing `.playerbots bot addclass` in the
game is the only way so far, and it gives a random spec.

Upstream facts (mod-playerbots @ 405f811, 2026-11-20):

- `.playerbots bot` (`add`, `addclass`, ...) is `Console::No`: it needs the
  player's own session. `.playerbots exec <bot> <command>` is `Console::Yes`
  but works only on a bot that is already online
  (`PlayerbotCommandScript.cpp`).
- `PlayerbotHolder::AddPlayerBot(guid, masterAccountId)` is public. The
  `addclass` pool (`RandomPlayerbotMgr::addclassCache`, keyed by
  `GetTeamClassIdx(alliance, class)`, `Playerbots.AddClassAccountPoolSize` = 50
  accounts) is public too. The login is asynchronous: `addclass` only answers
  "Add class <class>", without the bot's name.
- At login, `PlayerbotHolder::OnBotLogin` queues a `GroupInviteOperation`
  that puts the bot in its master's group (and turns a full party into a raid).
  An addclass bot more than 3 levels away from its master is re-rolled at the
  master's level with gear capped by the master's
  (`GetMixedGearScore * AutoInitEquipLevelLimitRatio`).
- `PlayerbotFactory(bot, level, quality, gearScoreLimit)` with `Randomize`,
  `InitTalentsBySpecNo(bot, specNo, reset)`, `InitEquipment`, `InitGlyphs`,
  `ApplyEnchantAndGemsNew`, `InitPet` and `InitAmmo` are public. Premade
  specs are `Playerbots.PremadeSpecName.<class>.<n>` (e.g. 1.2 = `prot pve`).
- The launcher talks to a running worldserver only through its console
  (ADR 0011, 0020). `.npc add` being `Console::No` already led to writing
  database rows instead (ADR 0023). That cannot work here: logging a bot in
  happens inside the server.

## Decision

1. **A module of our own**, `mod-realmkeeper`
   (`github.com/FayneDre/mod-realmkeeper`, GPL-2.0-or-later like
   mod-playerbots). It is pinned in the catalog like any module. It is required
   by the `playerbot` flavor (`removable: false`, `activation: always`) and has
   no SQL. It is compiled in the full build (ADR 0003), so the server build key
   changes and players get it through the usual server update. Its directory
   name is `mod-realmkeeper`, so its loader is `Addmod_realmkeeperScripts`.
2. **Console commands** (`Console::Yes`, GM level 3):
   - `.rk party add <player> <class> <spec number> <level>` picks a free
     character of the player's faction from the addclass pool and logs it in
     with `AddPlayerBot`. A world update hook then sets up the bot once it is
     in the world: `Randomize` at the level, premade spec, gear for the spec,
     enchants, pet, ammo, then `ResetStrategies`. At the player's own level the
     gear is capped like `addclass` does; at another level it follows the
     random bots' limits.
   - `.rk party remove <player>` sends away every bot the player controls.
   - `.rk party list <player>` lists them, set up or still joining.

   The commands refuse: a party of 5 (raids are out of scope), dungeon finder
   and battleground groups, a death knight under the heroic start level, a
   spec number with no premade spec.
3. **Reply protocol**: every reply line starts with `RKPARTY`:
   - `OK <bot>` or `ERROR <code>`;
   - `MEMBER <name> <class id> <level> <ready|joining>`, then `END <count>`;
   - `REMOVED <name>`, then `END <count>`.

   Error codes are stable and translated by the launcher (`party.<code>`). The
   module's README is the reference for this protocol.
4. **Manifest `party`**, on the module that provides it (data, not engine
   code):
   - `commands`: templates with `{player}`, `{class}`, `{spec}`, `{level}`;
   - `size`: the party size;
   - `classes`: id, Playerbots class token, localized name, minimum level, and
     PvE specs with their number, role (`tank`, `healer`, `dps`) and localized
     name.

   Another emulator offers parties by shipping a module with the same protocol
   and its own `party` data. Nothing in the engine names AzerothCore.
5. **Launcher**: `rk-launcher` sends a party command through the session that
   holds the running servers. This is the GM panel path: the console line,
   then waiting for the `RKPARTY` lines that answer it, with a timeout.
6. **UI**: the Bots tab gets sub-tabs `Settings | Party`. The Party page shows:
   - the played character: one of the online non-bot characters, picked
     automatically when there is only one;
   - a level, by default the character's;
   - up to 4 slots, each with a role, then a class and a spec offered for that
     role;
   - "Form the party", which adds the slots one by one;
   - the bots already there (refreshed with `list`);
   - "Send the bots away".

## Consequences

- One more upstream to maintain, ours. It is coupled to Playerbots' public
  API: a Playerbots pin bump must rebuild it, and a CI build failure points at
  it.
- Level choice works from the start because the module rolls the bot itself.
  Re-rolling at another level replaces the bot's gear, which is fine for
  addclass pool characters.
- The party only exists while the player is online: Playerbots logs a
  master's bots out when the master leaves.
- The launcher's class/spec list must match the pinned Playerbots premade
  spec numbers: `bump-pins` checks `PremadeSpecName` when Playerbots moves.
- Raids, race and gender choice, and PvP specs are left for later.

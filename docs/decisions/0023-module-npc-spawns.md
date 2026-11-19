# 0023. Module NPCs placed by the launcher

- Status: Accepted
- Date: 2026-11-19

## Context

Several modules only define their NPC as a creature template, and expect a
game master to place it in game:

- mod-transmog (template `190010`), mod-npc-enchanter (`601015`) and
  mod-reagent-bank (`290011`): their SQL has no `creature` row (module
  `data/sql/db-world` at the pinned commits, 2026-11-18).
- `.npc add` is `Console::No` in the fork (`cs_npc.cpp` @ Playerbot,
  2026-11-19): it needs a character in game, so the GM panel (ADR 0020),
  which sends console commands, cannot place them.
- Players of a public launcher would have to learn `.npc add 190010`, at
  a good spot, once per realm, and again after a world regeneration
  (ADR 0012). Without its NPC, mod-reagent-bank has no effect at all.

The world database has named, reachable positions: `game_tele` (the
`.tele` places, already read by the GM panel). On the fork it holds
`Stormwind` (map 0, -8833.38, 628.628, 94.0) and `Orgrimmar` (map 1,
1629.85, -4373.64, 31.6), among others. The TrinityCore family has the
same table. The fork's `creature` rows reach guid 5 300 678 with every
catalog module (local `pubtest`, 2026-11-19).

## Decision

1. **Manifest.** A module may declare `spawns`:
   `[{ "creature": <template id>, "places": ["Stormwind", "Orgrimmar"] }]`.
   Place names are `game_tele` names, ASCII letters and digits only
   (schema pattern, checked again by the driver before any SQL).
2. **When.** `EmulatorDriver::prepare_start`, before every start (the
   database runs, the servers do not). `DriverContext::spawns` holds the
   spawns of the enabled modules, in resolution order.
3. **AzerothCore driver.** In one transaction on the world database:
   - delete the launcher's rows (`creature.guid` 9 900 000 to 9 900 999,
     reserved for the launcher);
   - for each NPC and place, insert one row copied from the place's
     `game_tele` position, joined with `creature_template`: a missing place
     or template (module SQL not applied) inserts nothing, so the server
     never loads a spawn without its template;
   - NPCs at the same place stand 2.5 yards apart along x.
4. **Catalog.** Transmog, enchanter and reagent bank NPCs are placed in
   Stormwind and Orgrimmar.

## Consequences

- Enabling such a module places its NPC at the next start; disabling it
  removes the NPC at the next start (mod-reagent-bank then really is off).
  A world regeneration loses nothing: the rows come back at the next start.
- Spots are the teleport arrival points, not hand-picked: always reachable,
  but in a busy square. The x offset is not checked against the ground
  height; a spot that turns out bad is changed in the manifest.
- An NPC a GM moved in game (`.npc move`) is put back at each start. A GM
  who wants another spot places their own copy with `.npc add`.
- A driver for another emulator implements the same idea with its own
  tables and guid range; the manifest field stays the same.
- Verified: generated SQL on the `pubtest` world database (row at
  Stormwind + 2.5, nothing for a missing template or place). In-game check
  of the three NPCs: needs a build with these modules (0.3.2).

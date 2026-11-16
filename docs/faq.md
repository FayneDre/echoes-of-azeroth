# RealmKeeper: FAQ and troubleshooting

[Version française](faq.fr.md)

Every error message in the launcher has a **Help** link to the matching
section below. If nothing here helps, use **Report a problem** at the bottom
of the launcher. Click **Copy the diagnostic** first and paste it into the
report. Your personal folders are hidden in it.

- [Before you start](#start)
- [The game folder](#client)
- [Install location and disk space](#location)
- [Antivirus and missing server files](#antivirus)
- [Ports and other servers](#ports)
- [Downloads and the Internet](#network)
- [An interrupted or failed change](#operations)
- [Servers that do not start](#servers)
- [Backups and restores](#backups)
- [Several realms, moving to another computer](#realms)
- [Playing on a local network](#lan)
- [GM panel and accounts](#gm)
- [Something else](#other)

<a id="start"></a>
## Before you start

- **What do I need?** Windows 10 or 11 (64-bit), about 10 GB of free disk
  space per realm, and your own World of Warcraft client for the version you
  want to play (3.3.5a, build 12340, for Wrath of the Lich King). The
  launcher never downloads or shares the game client.
- **Do I need administrator rights?** No. Everything is installed in your
  user folder or in the folder you pick. Only the optional rule that opens
  the firewall for local network play asks for elevation.
- **Is the game modified?** The launcher writes the realm address, and only
  adds the files of the module options you turn on (for example a client
  patch or the MultiBot add-on). These files are removed when you turn the
  option off. Your own files are moved aside and put back.

<a id="client"></a>
## The game folder

- **"No game found in this folder"**: choose the folder that contains
  `Wow.exe`, not its `Data` folder or a shortcut. The launcher also looks
  for the game on this computer by itself and offers what it finds
  (**Use this one**).
- **"This game is build …"**: each realm needs one exact client version
  (3.3.5a is build 12340). Other builds cannot connect.
- **"The launcher cannot write in this game folder"**: games under
  `C:\Program Files` are protected by Windows. Move the game folder elsewhere
  (for example `C:\Games`), then choose it again.
- **"A file in your game folder would be replaced"**: the details name the
  file. Move it out of the game folder (keep it if it is yours), then try
  again.
- **"A game file is in use"**: close the game, then try again.

<a id="location"></a>
## Install location and disk space

- **Which folder should I pick?** An empty folder on an internal drive
  formatted in NTFS (the usual Windows format), for example
  `D:\RealmKeeper`.
- **"… is synchronised by OneDrive / Dropbox / Google Drive"**: a database
  inside a synchronised folder ends up corrupted, because files are uploaded
  while the server writes them. Windows often puts "Documents" and "Desktop"
  in OneDrive: pick a folder outside them.
- **"… cannot hold the server files"**: USB sticks in FAT32/exFAT and most
  network drives cannot hold the folder links the servers need. Choose an
  internal NTFS drive.
- **"Not enough disk space"**: the message gives the space needed and the
  space free. Free some space, or choose another location before the first
  install. The launcher checks before it downloads anything.
- **I renamed or moved the folder.** On the next start, the launcher asks
  you to locate it. A realm that was moved shows **Update the paths** on its
  Home tab.

<a id="antivirus"></a>
## Antivirus and missing server files

Server programs are not well-known applications, so an antivirus sometimes
quarantines them (`worldserver.exe`, `authserver.exe`, `legacy.dll`, …).

1. Open your antivirus (Windows Security › Virus & threat protection ›
   Protection history) and restore the files it removed. You can also skip
   this step.
2. Add an **exclusion** for the RealmKeeper folder (Windows Security ›
   Virus & threat protection › Manage settings › Exclusions).
3. In the launcher, click **Repair** (Home › Maintenance). It downloads and
   checks the server files again. Your characters and accounts are kept.

The launcher downloads only files whose signature or checksum it verified.

<a id="ports"></a>
## Ports and other servers

A realm listens on ports 3724 (login), 8085 (world) and 7878 (SOAP), plus a
private database port from 3307 up.

- **"Port … is used by …"**: another program holds the port. It is often
  another WoW server, or a second realm of this launcher that is still
  running. Close it, then try again. Only one realm runs at a time.
- **"No free network port was found"**: close other database servers, then
  try again.

<a id="network"></a>
## Downloads and the Internet

- **"A download failed"**: check your connection, then run the same action
  again. Downloads resume where they stopped and are verified when they
  finish.
- **"The list of server builds could not be verified"**: the signed list of
  server builds could not be fetched or its signature is wrong. Try again
  later. If a proxy or a firewall filters GitHub, allow `github.com` and
  `objects.githubusercontent.com`.
- **Does the game need the Internet?** No. Once installed, a realm runs
  offline. Installing, updating and importing need the Internet.

<a id="operations"></a>
## An interrupted or failed change

Installs, updates, module changes, restores and imports are done step by
step. Each step is recorded, so nothing is lost when one stops.

- **"Cancelled" / "A step failed" / a closed launcher / a power cut**: run
  the same action again, or click **Resume** on the Home tab. It continues
  with the same plan.
- **"The change failed and was undone"**: everything is as before. Try again.
  If it fails again, report the problem with the technical details.
- **"… could not be fully undone"**: do not try again. Report the problem
  with the technical details and the diagnostic. Backups made before the
  change are in the Backups tab.
- **Can my computer sleep during an install?** The launcher keeps it awake
  while it works.

<a id="servers"></a>
## Servers that do not start

- **"… the start failed. Its console shows why."**: open **Terminals** at the
  bottom of the launcher and read the last lines of that server.
  - A database error means another program uses its port, or the disk is
    full.
  - Missing files: see [antivirus](#antivirus).
- **The world server takes a long time.** With 500 bots it takes 1 to 4
  minutes. The bar under "Servers" shows how far it got.
- **"This realm's folder was renamed or moved"**: click **Update the
  paths** on the Home tab.
- **Shutting Windows down with the servers running**: the launcher stops
  them cleanly first, so the world is saved. Windows shows why it waits,
  for up to a minute.

<a id="backups"></a>
## Backups and restores

- **What is backed up?** Accounts, characters and bots: the player data.
  The game world itself is rebuilt from the server build.
- **When?** A daily backup is made when the servers stop (if enabled), and
  a full one is made before every update, repair and restore. You can also
  click **Back up now**.
- **Restoring** puts the player data back as the backup holds it. The
  current state is backed up first, so a restore can be undone.
- **"This file holds no player data of this realm"**: the file comes from
  another emulator, or it is not a backup.

<a id="realms"></a>
## Several realms, moving to another computer

- **Duplicate** (Home, above the realm) copies a stopped realm with its
  modules, settings, accounts and characters. Use it to try other modules
  without risk. The copy takes the size of its database on the disk (1-2
  GB). Only one realm runs at a time.
- **Export** writes a stopped realm to one `.rkrealm` file: modules,
  settings, accounts and characters, without the game world, which is
  rebuilt.
- **Import a realm** on the other computer installs a new realm from that
  file, then puts its accounts and characters back. It needs the Internet,
  like a first install.
- **"This realm uses …, which this launcher does not offer"**: update the
  launcher on this computer first.
- **"This file is not a realm exported by RealmKeeper"**: the file is
  damaged or of another kind. Export it again.
- An export holds account names and their password checks (not the
  passwords themselves). Share it only like a save file.

<a id="lan"></a>
## Playing on a local network

- The host installs the realm with **Local network** chosen. Friends click
  **Join a game on the local network** and enter the host's address (shown
  on the host's Home tab, for example `192.168.1.20`).
- If friends cannot connect, the host's Windows firewall probably blocks
  ports 3724 and 8085. Allow `authserver.exe` and `worldserver.exe` for
  private networks when Windows asks, or add the rule by hand.
- Module options that add client files (patches, add-ons) must also be
  installed on the friends' game folders.

<a id="gm"></a>
## GM panel and accounts

- Create your account in the **Account** tab. A GM level above 0 gives
  access to GM commands in the game. The new level applies at your next
  login.
- The **Account** tab, **Game master** part, sends ready-made commands (teleport, level,
  revive, send an item or gold, announce) while the servers run. The values
  are checked first.

<a id="other"></a>
## Something else

Use **Report a problem** at the bottom of the launcher. Click **Copy the
diagnostic** first and paste it into the report: it gives the versions, the
realms and the last log lines, with your personal folders hidden.

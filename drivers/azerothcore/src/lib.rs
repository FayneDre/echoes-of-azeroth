//! AzerothCore driver (ADR 0004): what the manifest cannot declare.
//!
//! - **Accounts** are written to the auth database directly, exactly as
//!   `AccountMgr::CreateAccount` does (SRP6 salt + verifier, upper-cased
//!   name and password, `realmcharacters` rows, `account_access` for GM
//!   levels). No running worldserver is needed.
//! - **Realm list:** before each start, realm 1 (`RealmID` in
//!   `worldserver.conf`) points at the current address and world port, so a
//!   changed LAN address never strands the players.
//! - **Characters** without a client (the AHBot seller): one `characters`
//!   row with the next guid, written while worldserver is stopped (it reads
//!   the highest guid at startup). mod-ah-bot never loads the character, it
//!   only uses the guid as auction owner (`AuctionHouseBot.cpp` builds an
//!   in-memory `Player` with `Initialize(guid)`).
//! - **Population:** characters flagged `online` in the characters
//!   database; those of `RNDBOT` accounts (Playerbots'
//!   `RandomBotAccountPrefix`, upper-cased like every account name) are bots.

#![forbid(unsafe_code)]

pub mod srp6;

use std::fmt::Write as _;

use rk_runtime::{
    AccountInfo, DriverContext, DriverFuture, EmulatorDriver, NewAccount, NewCharacter,
    OnlineCharacter, Population,
};
use thiserror::Error;

/// `MAX_ACCOUNT_STR` / `MAX_PASS_STR` in `AccountMgr.h`.
pub const MAX_USERNAME: usize = 17;
pub const MAX_PASSWORD: usize = 16;
/// `SEC_ADMINISTRATOR`; `SEC_CONSOLE` (4) is reserved for the console.
pub const MAX_GM_LEVEL: u8 = 3;
/// Wrath of the Lich King, the fork's `Expansion` default.
const EXPANSION: u8 = 2;
/// `RealmID` in `worldserver.conf.dist`, which the launcher does not change.
const REALM_ID: u32 = 1;
/// `Playerbots.RandomBotAccountPrefix` default (`rndbot`), upper-cased by
/// `AccountMgr`; the launcher does not change it.
const BOT_ACCOUNT_PREFIX: &str = "RNDBOT";

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("the account name must be 1 to {MAX_USERNAME} letters or digits")]
    InvalidName,
    #[error("the password must be 1 to {MAX_PASSWORD} characters, without spaces")]
    InvalidPassword,
    #[error("the GM level must be between 0 and {MAX_GM_LEVEL}")]
    InvalidGmLevel,
    #[error("the account '{0}' already exists")]
    Exists(String),
    #[error("no such account")]
    UnknownAccount,
    #[error("character names are 2 to 12 letters, the first one upper-case")]
    InvalidCharacterName,
    #[error("unknown race or class '{0}'")]
    UnknownRaceOrClass(String),
    #[error("the character name '{0}' is already taken")]
    CharacterExists(String),
    #[error("the emulator manifest declares no '{0}' database")]
    MissingDatabase(&'static str),
    #[error("could not generate a random salt: {0}")]
    Random(String),
    #[error("unexpected answer from the database: {0:?}")]
    Unexpected(String),
    #[error(transparent)]
    Db(#[from] rk_db::DbError),
}

pub struct AzerothCore;

impl EmulatorDriver for AzerothCore {
    fn id(&self) -> &str {
        "azerothcore"
    }

    fn prepare_start<'a>(&'a self, cx: &'a DriverContext<'a>) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let auth = cx
                .database("auth")
                .ok_or(AccountError::MissingDatabase("auth"))?;
            let port = cx.port("world").ok_or("no 'world' port in the instance")?;
            let icon = realm_icon(&cx.configs.join("worldserver.conf"));
            cx.db
                .execute(&realm_sql(auth, cx.realm_address, port, icon))
                .await?;
            Ok(())
        })
    }

    fn create_account<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        account: &'a NewAccount,
    ) -> DriverFuture<'a, u32> {
        Box::pin(async move {
            let auth = cx
                .database("auth")
                .ok_or(AccountError::MissingDatabase("auth"))?;
            let (username, password) = normalize(account)?;
            let exists = cx
                .db
                .execute(&format!(
                    "SELECT COUNT(*) FROM `{auth}`.account WHERE username = '{username}';"
                ))
                .await?;
            if exists.trim() != "0" {
                return Err(AccountError::Exists(username).into());
            }
            let mut salt = [0u8; srp6::SALT_LENGTH];
            getrandom::fill(&mut salt).map_err(|e| AccountError::Random(e.to_string()))?;
            let verifier = srp6::verifier(&username, &password, &salt);
            let out = cx
                .db
                .execute(&account_sql(
                    auth,
                    &username,
                    &salt,
                    &verifier,
                    account.gm_level,
                ))
                .await?;
            let id = out.trim();
            Ok(id
                .parse()
                .map_err(|_| AccountError::Unexpected(id.to_owned()))?)
        })
    }

    fn find_account<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        username: &'a str,
    ) -> DriverFuture<'a, Option<u32>> {
        Box::pin(async move {
            let auth = cx
                .database("auth")
                .ok_or(AccountError::MissingDatabase("auth"))?;
            let probe = NewAccount {
                username: username.to_owned(),
                password: "x".into(),
                gm_level: 0,
            };
            let (username, _) = normalize(&probe)?;
            let out = cx
                .db
                .execute(&format!(
                    "SELECT id FROM `{auth}`.account WHERE username = '{username}';"
                ))
                .await?;
            parse_optional(&out)
        })
    }

    fn create_character<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        character: &'a NewCharacter,
    ) -> DriverFuture<'a, u32> {
        Box::pin(async move {
            let chars = cx
                .database("characters")
                .ok_or(AccountError::MissingDatabase("characters"))?;
            check_character_name(&character.name)?;
            let race = lookup(RACES, character.race.as_deref().unwrap_or("human"))?;
            let class = lookup(CLASSES, character.class.as_deref().unwrap_or("warrior"))?;
            let taken = cx
                .db
                .execute(&format!(
                    "SELECT COUNT(*) FROM `{chars}`.characters WHERE name = '{}';",
                    character.name
                ))
                .await?;
            if taken.trim() != "0" {
                return Err(AccountError::CharacterExists(character.name.clone()).into());
            }
            let out = cx
                .db
                .execute(&character_sql(chars, character, race, class))
                .await?;
            let guid = out.trim();
            Ok(guid
                .parse()
                .map_err(|_| AccountError::Unexpected(guid.to_owned()))?)
        })
    }

    fn find_character<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        name: &'a str,
    ) -> DriverFuture<'a, Option<(u32, u32)>> {
        Box::pin(async move {
            let chars = cx
                .database("characters")
                .ok_or(AccountError::MissingDatabase("characters"))?;
            check_character_name(name)?;
            let out = cx
                .db
                .execute(&format!(
                    "SELECT account, guid FROM `{chars}`.characters WHERE name = '{name}';"
                ))
                .await?;
            let line = out.trim();
            if line.is_empty() {
                return Ok(None);
            }
            let mut fields = line.split('\t').map(str::parse::<u32>);
            match (fields.next(), fields.next()) {
                (Some(Ok(account)), Some(Ok(guid))) => Ok(Some((account, guid))),
                _ => Err(AccountError::Unexpected(line.to_owned()).into()),
            }
        })
    }

    fn accounts<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
    ) -> DriverFuture<'a, Option<Vec<AccountInfo>>> {
        Box::pin(async move {
            let (auth, _) = Self::names(cx)?;
            let out = cx.db.execute(&accounts_sql(auth)).await?;
            Ok(Some(parse_accounts(&out)?))
        })
    }

    fn set_gm_level<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        username: &'a str,
        gm_level: u8,
    ) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            if gm_level > MAX_GM_LEVEL {
                return Err(AccountError::InvalidGmLevel.into());
            }
            let id = self
                .find_account(cx, username)
                .await?
                .ok_or(AccountError::UnknownAccount)?;
            let (auth, _) = Self::names(cx)?;
            cx.db.execute(&gm_level_sql(auth, id, gm_level)).await?;
            Ok(())
        })
    }

    fn online_characters<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        limit: u32,
    ) -> DriverFuture<'a, Option<Vec<OnlineCharacter>>> {
        Box::pin(async move {
            let (auth, chars) = Self::names(cx)?;
            let out = cx.db.execute(&online_sql(auth, chars, limit)).await?;
            Ok(Some(parse_online(&out)?))
        })
    }

    fn teleport_locations<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
    ) -> DriverFuture<'a, Vec<String>> {
        Box::pin(async move {
            let world = cx
                .database("world")
                .ok_or(AccountError::MissingDatabase("world"))?;
            let out = cx
                .db
                .execute(&format!(
                    "SELECT DISTINCT name FROM `{world}`.game_tele ORDER BY name;"
                ))
                .await?;
            // Upstream has a few duplicate names (one entry each). Only
            // names the console accepts as one argument: a place
            // added with a space in its name could not be used.
            Ok(out
                .lines()
                .map(str::trim)
                .filter(|name| rk_core::console::valid_location(name))
                .map(str::to_owned)
                .collect())
        })
    }

    fn population<'a>(&'a self, cx: &'a DriverContext<'a>) -> DriverFuture<'a, Option<Population>> {
        Box::pin(async move {
            let auth = cx
                .database("auth")
                .ok_or(AccountError::MissingDatabase("auth"))?;
            let chars = cx
                .database("characters")
                .ok_or(AccountError::MissingDatabase("characters"))?;
            let out = cx.db.execute(&population_sql(auth, chars)).await?;
            Ok(Some(parse_population(&out)?))
        })
    }
}

impl AzerothCore {
    fn names<'a>(cx: &'a DriverContext<'a>) -> Result<(&'a str, &'a str), AccountError> {
        let auth = cx
            .database("auth")
            .ok_or(AccountError::MissingDatabase("auth"))?;
        let chars = cx
            .database("characters")
            .ok_or(AccountError::MissingDatabase("characters"))?;
        Ok((auth, chars))
    }
}

/// Player accounts with their highest GM level on any realm.
fn accounts_sql(auth: &str) -> String {
    format!(
        "SELECT a.id, a.username, COALESCE(MAX(x.gmlevel), 0) FROM `{auth}`.account a \
         LEFT JOIN `{auth}`.account_access x ON x.id = a.id \
         WHERE a.username NOT LIKE '{BOT_ACCOUNT_PREFIX}%' GROUP BY a.id, a.username ORDER BY a.username;"
    )
}

fn parse_accounts(out: &str) -> Result<Vec<AccountInfo>, AccountError> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            match (fields.next(), fields.next(), fields.next()) {
                (Some(id), Some(name), Some(level)) => Ok(AccountInfo {
                    id: id
                        .parse()
                        .map_err(|_| AccountError::Unexpected(line.into()))?,
                    username: name.to_owned(),
                    gm_level: level
                        .parse()
                        .map_err(|_| AccountError::Unexpected(line.into()))?,
                }),
                _ => Err(AccountError::Unexpected(line.to_owned())),
            }
        })
        .collect()
}

/// Replaces the account's GM level on every realm (`-1`), like
/// `account set gmlevel <name> <level> -1`.
fn gm_level_sql(auth: &str, id: u32, gm_level: u8) -> String {
    let mut sql =
        format!("USE `{auth}`;\nSTART TRANSACTION;\nDELETE FROM account_access WHERE id = {id};\n");
    if gm_level > 0 {
        let _ = writeln!(
            sql,
            "INSERT INTO account_access (id, gmlevel, RealmID) VALUES ({id}, {gm_level}, -1);"
        );
    }
    sql.push_str("COMMIT;\n");
    sql
}

/// Characters in the world, players first.
fn online_sql(auth: &str, chars: &str, limit: u32) -> String {
    format!(
        "SELECT c.name, c.level, c.race, c.class, a.username LIKE '{BOT_ACCOUNT_PREFIX}%' AS bot \
         FROM `{chars}`.characters c JOIN `{auth}`.account a ON a.id = c.account \
         WHERE c.online <> 0 ORDER BY bot, c.name LIMIT {limit};"
    )
}

fn parse_online(out: &str) -> Result<Vec<OnlineCharacter>, AccountError> {
    let name_of = |table: &[(&str, u8)], id: &str| {
        id.parse::<u8>()
            .ok()
            .and_then(|id| table.iter().find(|(_, v)| *v == id))
            .map_or_else(|| id.to_owned(), |(n, _)| (*n).to_owned())
    };
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [name, level, race, class, bot] = f[..] else {
                return Err(AccountError::Unexpected(line.to_owned()));
            };
            Ok(OnlineCharacter {
                name: name.to_owned(),
                level: level
                    .parse()
                    .map_err(|_| AccountError::Unexpected(line.into()))?,
                race: name_of(RACES, race),
                class: name_of(CLASSES, class),
                bot: bot.trim() == "1",
            })
        })
        .collect()
}

/// Online characters split between player and bot accounts.
fn population_sql(auth: &str, chars: &str) -> String {
    format!(
        "SELECT COALESCE(SUM(a.username NOT LIKE '{BOT_ACCOUNT_PREFIX}%'), 0), \
         COALESCE(SUM(a.username LIKE '{BOT_ACCOUNT_PREFIX}%'), 0) \
         FROM `{chars}`.characters c JOIN `{auth}`.account a ON a.id = c.account \
         WHERE c.online <> 0;"
    )
}

fn parse_population(out: &str) -> Result<Population, AccountError> {
    let line = out.trim();
    let mut fields = line.split('\t').map(str::parse::<u32>);
    match (fields.next(), fields.next()) {
        (Some(Ok(players)), Some(Ok(bots))) => Ok(Population { players, bots }),
        _ => Err(AccountError::Unexpected(line.to_owned())),
    }
}

/// `ChrRaces.dbc` / `ChrClasses.dbc` ids of 3.3.5a.
const RACES: &[(&str, u8)] = &[
    ("human", 1),
    ("orc", 2),
    ("dwarf", 3),
    ("night-elf", 4),
    ("undead", 5),
    ("tauren", 6),
    ("gnome", 7),
    ("troll", 8),
    ("blood-elf", 10),
    ("draenei", 11),
];
const CLASSES: &[(&str, u8)] = &[
    ("warrior", 1),
    ("paladin", 2),
    ("hunter", 3),
    ("rogue", 4),
    ("priest", 5),
    ("death-knight", 6),
    ("shaman", 7),
    ("mage", 8),
    ("warlock", 9),
    ("druid", 11),
];

fn lookup(table: &[(&str, u8)], name: &str) -> Result<u8, AccountError> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, id)| *id)
        .ok_or_else(|| AccountError::UnknownRaceOrClass(name.to_owned()))
}

/// The client's rule for new names (also keeps the value safe in SQL).
fn check_character_name(name: &str) -> Result<(), AccountError> {
    let mut chars = name.chars();
    let ok = (2..=12).contains(&name.len())
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_lowercase());
    if ok {
        Ok(())
    } else {
        Err(AccountError::InvalidCharacterName)
    }
}

/// One level-1 character at a human start position (only relevant if
/// someone ever logs it in).
fn character_sql(chars: &str, c: &NewCharacter, race: u8, class: u8) -> String {
    let mut sql = String::new();
    let _ = writeln!(sql, "USE `{chars}`;");
    sql.push_str("START TRANSACTION;\n");
    sql.push_str("SELECT COALESCE(MAX(guid), 0) + 1 INTO @guid FROM characters FOR UPDATE;\n");
    let _ = writeln!(
        sql,
        "INSERT INTO characters (guid, account, name, race, class, gender, level, money, \
         map, zone, position_x, position_y, position_z, orientation, taximask, innTriggerId, at_login) \
         VALUES (@guid, {}, '{}', {race}, {class}, 0, 1, 0, 0, 12, -8949.95, -132.493, 83.5312, 0, '', 0, 0);",
        c.account_id, c.name
    );
    sql.push_str("COMMIT;\nSELECT @guid;\n");
    sql
}

fn parse_optional(out: &str) -> Result<Option<u32>, rk_runtime::DriverError> {
    let line = out.trim();
    if line.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        line.parse()
            .map_err(|_| AccountError::Unexpected(line.to_owned()))?,
    ))
}

/// Validates and upper-cases like `Utf8ToUpperOnlyLatin` (ASCII only here,
/// so the values are safe inside SQL literals).
fn normalize(account: &NewAccount) -> Result<(String, String), AccountError> {
    let name = &account.username;
    if name.is_empty()
        || name.len() > MAX_USERNAME
        || !name.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(AccountError::InvalidName);
    }
    let pass = &account.password;
    if pass.is_empty() || pass.len() > MAX_PASSWORD || !pass.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(AccountError::InvalidPassword);
    }
    if account.gm_level > MAX_GM_LEVEL {
        return Err(AccountError::InvalidGmLevel);
    }
    Ok((name.to_ascii_uppercase(), pass.to_ascii_uppercase()))
}

/// The statements of `AccountMgr::CreateAccount` (+ `account_access` for a GM
/// level), in one transaction; prints the new account id.
fn account_sql(auth: &str, username: &str, salt: &[u8], verifier: &[u8], gm_level: u8) -> String {
    let mut sql = String::new();
    let _ = writeln!(sql, "USE `{auth}`;");
    sql.push_str("START TRANSACTION;\n");
    let _ = writeln!(
        sql,
        "INSERT INTO account (username, salt, verifier, expansion, reg_mail, email, joindate) \
         VALUES ('{username}', X'{}', X'{}', {EXPANSION}, '', '', NOW());",
        hex::encode(salt),
        hex::encode(verifier)
    );
    sql.push_str("SET @id = LAST_INSERT_ID();\n");
    sql.push_str(
        "INSERT INTO realmcharacters (realmid, acctid, numchars) \
         SELECT realmlist.id, account.id, 0 FROM realmlist, account \
         LEFT JOIN realmcharacters ON acctid = account.id WHERE acctid IS NULL;\n",
    );
    if gm_level > 0 {
        let _ = writeln!(
            sql,
            "INSERT INTO account_access (id, gmlevel, RealmID) VALUES (@id, {gm_level}, -1);"
        );
    }
    sql.push_str("COMMIT;\nSELECT @id;\n");
    sql
}

/// Realm type shown in the client's realm list: worldserver's `GameType`
/// (0 normal, 1 PvP, 4 normal, 6 RP, 8 RP-PvP, 16 FFA PvP). The authserver
/// sends `realmlist.icon`, which worldserver does not update, so it is
/// copied before each start. Unknown or unreadable: 0.
fn realm_icon(worldserver_conf: &std::path::Path) -> u8 {
    std::fs::read_to_string(worldserver_conf)
        .ok()
        .and_then(|text| {
            rk_conf::ConfFile::parse(&text)
                .get("GameType")
                .and_then(|v| v.trim().trim_matches('"').parse().ok())
        })
        .filter(|icon| matches!(icon, 0 | 1 | 4 | 6 | 8 | 16))
        .unwrap_or(0)
}

/// Points realm 1 at the address players connect to, with its type.
fn realm_sql(auth: &str, address: std::net::Ipv4Addr, port: u16, icon: u8) -> String {
    format!(
        "INSERT INTO `{auth}`.realmlist (id, name, address, localAddress, port, icon) \
         VALUES ({REALM_ID}, 'AzerothCore', '{address}', '{address}', {port}, {icon}) AS new \
         ON DUPLICATE KEY UPDATE address = new.address, localAddress = new.localAddress, \
         port = new.port, icon = new.icon;\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(username: &str, password: &str, gm_level: u8) -> NewAccount {
        NewAccount {
            username: username.into(),
            password: password.into(),
            gm_level,
        }
    }

    #[test]
    fn population_counts_players_and_bots() {
        let sql = population_sql("acore_auth", "acore_characters");
        assert!(sql.contains("`acore_characters`.characters c JOIN `acore_auth`.account a"));
        assert!(sql.contains("LIKE 'RNDBOT%'"));
        assert_eq!(
            parse_population("1\t487\n").unwrap(),
            Population {
                players: 1,
                bots: 487
            }
        );
        assert!(parse_population("NULL\tNULL").is_err());
    }

    #[test]
    fn realm_icon_follows_game_type() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("worldserver.conf");
        assert_eq!(realm_icon(&conf), 0, "missing file");
        std::fs::write(
            &conf,
            "# realm
GameType = 8
",
        )
        .unwrap();
        assert_eq!(realm_icon(&conf), 8);
        std::fs::write(
            &conf,
            "GameType = 3
",
        )
        .unwrap();
        assert_eq!(realm_icon(&conf), 0, "not a realm type");
        assert!(
            realm_sql("acore_auth", std::net::Ipv4Addr::LOCALHOST, 8085, 1)
                .contains("icon = new.icon")
        );
    }

    #[test]
    fn names_and_passwords_are_checked_and_upper_cased() {
        assert_eq!(
            normalize(&account("Player1", "s3cr3t!", 0)).unwrap(),
            ("PLAYER1".into(), "S3CR3T!".into())
        );
        for bad in ["", "has space", "quote'", "é", "abcdefghijklmnopqr"] {
            assert!(
                matches!(
                    normalize(&account(bad, "x", 0)),
                    Err(AccountError::InvalidName)
                ),
                "{bad}"
            );
        }
        for bad in ["", "with space", "abcdefghijklmnopq", "pässe"] {
            assert!(
                matches!(
                    normalize(&account("a", bad, 0)),
                    Err(AccountError::InvalidPassword)
                ),
                "{bad}"
            );
        }
        assert!(matches!(
            normalize(&account("a", "b", 4)),
            Err(AccountError::InvalidGmLevel)
        ));
    }

    #[test]
    fn character_names_races_and_classes_are_checked() {
        assert!(check_character_name("Auctioneer").is_ok());
        for bad in ["A", "auctioneer", "AuctioneerToo", "Auc'tion", "Ébène"] {
            assert!(check_character_name(bad).is_err(), "{bad}");
        }
        assert_eq!(lookup(RACES, "blood-elf").unwrap(), 10);
        assert_eq!(lookup(CLASSES, "druid").unwrap(), 11);
        assert!(lookup(CLASSES, "monk").is_err());
        let character = NewCharacter {
            account_id: 5,
            name: "Auctioneer".into(),
            race: None,
            class: None,
        };
        let sql = character_sql("acore_characters", &character, 1, 1);
        assert!(
            sql.contains("VALUES (@guid, 5, 'Auctioneer', 1, 1,"),
            "{sql}"
        );
    }

    #[test]
    fn account_sql_grants_gm_level_only_when_asked() {
        let sql = account_sql("acore_auth", "P", &[0xab; 32], &[0xcd; 32], 0);
        assert!(sql.contains(&format!("X'{}'", "ab".repeat(32))));
        assert!(!sql.contains("account_access"));
        let sql = account_sql("acore_auth", "P", &[0; 32], &[0; 32], 3);
        assert!(sql.contains("VALUES (@id, 3, -1)"));
    }
}

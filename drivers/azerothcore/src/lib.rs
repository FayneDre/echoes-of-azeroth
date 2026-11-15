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

#![forbid(unsafe_code)]

pub mod srp6;

use std::fmt::Write as _;

use eoa_runtime::{DriverContext, DriverFuture, EmulatorDriver, NewAccount, NewCharacter};
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
    Db(#[from] eoa_db::DbError),
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
            cx.db
                .execute(&realm_sql(auth, cx.realm_address, port))
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

fn parse_optional(out: &str) -> Result<Option<u32>, eoa_runtime::DriverError> {
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

/// Points realm 1 at the address players connect to.
fn realm_sql(auth: &str, address: std::net::Ipv4Addr, port: u16) -> String {
    format!(
        "INSERT INTO `{auth}`.realmlist (id, name, address, localAddress, port) \
         VALUES ({REALM_ID}, 'AzerothCore', '{address}', '{address}', {port}) AS new \
         ON DUPLICATE KEY UPDATE address = new.address, localAddress = new.localAddress, \
         port = new.port;\n"
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

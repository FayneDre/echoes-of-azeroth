//! LoA Pandaria driver (Mists of Pandaria 5.4.8, ADR 0024): what the
//! manifest cannot declare. Prototype of a second emulator.
//!
//! - **Accounts** are written to the auth database as
//!   `AccountMgr::CreateAccount` does: upper-cased name and password,
//!   `sha_pass_hash` = hex SHA-1 of `NAME:PASSWORD`, `realmcharacters` rows,
//!   `account_access` for GM levels. `v` and `s` stay empty: the authserver
//!   computes them from `sha_pass_hash` at the first logon
//!   (`AuthSession::SetVSFields`). No running server is needed.
//! - **Realm list:** before each start, realm 1 (`RealmID` in
//!   `worldserver.conf`) points at the current address and world port, with
//!   the client build and worldserver's `GameType` as icon.
//! - **Population:** characters flagged `online`; those of `RNDBOT`
//!   accounts (`AiPlayerbot.RandomBotAccountPrefix`, upper-cased like every
//!   account name) are bots.
//! - **Characters** without a client are not supported (no module needs one).

#![forbid(unsafe_code)]

use std::fmt::Write as _;

use rk_runtime::{
    AccountInfo, DriverContext, DriverFuture, EmulatorDriver, NewAccount, NewCharacter,
    OnlineCharacter, Population,
};
use sha1::{Digest, Sha1};
use thiserror::Error;

/// `MAX_ACCOUNT_STR` / `MAX_PASS_STR` in `AccountMgr.h` (16 each).
pub const MAX_USERNAME: usize = 16;
pub const MAX_PASSWORD: usize = 16;
/// `SEC_ADMINISTRATOR`; `SEC_CONSOLE` (4) is reserved for the console.
pub const MAX_GM_LEVEL: u8 = 3;
/// `RealmID` in `worldserver.conf.dist`, which the launcher does not change.
const REALM_ID: u32 = 1;
/// Client build of 5.4.8, sent in the realm list.
const GAME_BUILD: u32 = 18414;
/// `AiPlayerbot.RandomBotAccountPrefix` default (`rndbot`), upper-cased.
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
    #[error("this emulator cannot create characters without a game client")]
    CharactersUnsupported,
    #[error("the emulator manifest declares no '{0}' database")]
    MissingDatabase(&'static str),
    #[error("unexpected answer from the database: {0:?}")]
    Unexpected(String),
    #[error(transparent)]
    Db(#[from] rk_db::DbError),
}

pub struct Pandaria;

impl Pandaria {
    fn auth<'a>(cx: &'a DriverContext<'a>) -> Result<&'a str, AccountError> {
        cx.database("auth")
            .ok_or(AccountError::MissingDatabase("auth"))
    }

    fn characters<'a>(cx: &'a DriverContext<'a>) -> Result<&'a str, AccountError> {
        cx.database("characters")
            .ok_or(AccountError::MissingDatabase("characters"))
    }
}

impl EmulatorDriver for Pandaria {
    fn id(&self) -> &str {
        "pandaria"
    }

    fn prepare_start<'a>(&'a self, cx: &'a DriverContext<'a>) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let auth = Self::auth(cx)?;
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
            let auth = Self::auth(cx)?;
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
            let hash = sha_pass_hash(&username, &password);
            let out = cx
                .db
                .execute(&account_sql(auth, &username, &hash, account.gm_level))
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
            let auth = Self::auth(cx)?;
            let username = normalize_name(username)?;
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
        _cx: &'a DriverContext<'a>,
        _character: &'a NewCharacter,
    ) -> DriverFuture<'a, u32> {
        Box::pin(async { Err(AccountError::CharactersUnsupported.into()) })
    }

    fn find_character<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        name: &'a str,
    ) -> DriverFuture<'a, Option<(u32, u32)>> {
        Box::pin(async move {
            let chars = Self::characters(cx)?;
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
            let out = cx.db.execute(&accounts_sql(Self::auth(cx)?)).await?;
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
            cx.db
                .execute(&gm_level_sql(Self::auth(cx)?, id, gm_level))
                .await?;
            Ok(())
        })
    }

    fn online_characters<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        limit: u32,
    ) -> DriverFuture<'a, Option<Vec<OnlineCharacter>>> {
        Box::pin(async move {
            let sql = online_sql(Self::auth(cx)?, Self::characters(cx)?, limit);
            let out = cx.db.execute(&sql).await?;
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
            let sql = population_sql(Self::auth(cx)?, Self::characters(cx)?);
            let out = cx.db.execute(&sql).await?;
            Ok(Some(parse_population(&out)?))
        })
    }
}

/// `AccountMgr::CalculateShaPassHash` on normalized values: upper-case hex
/// SHA-1 of `NAME:PASSWORD`.
pub fn sha_pass_hash(username: &str, password: &str) -> String {
    let digest = Sha1::new()
        .chain_update(username.as_bytes())
        .chain_update(b":")
        .chain_update(password.as_bytes())
        .finalize();
    hex::encode_upper(digest)
}

fn normalize_name(name: &str) -> Result<String, AccountError> {
    if name.is_empty()
        || name.len() > MAX_USERNAME
        || !name.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(AccountError::InvalidName);
    }
    Ok(name.to_ascii_uppercase())
}

/// Validates and upper-cases like `AccountMgr::normalizeString` (ASCII
/// only here, so the values are safe inside SQL literals).
fn normalize(account: &NewAccount) -> Result<(String, String), AccountError> {
    let name = normalize_name(&account.username)?;
    let pass = &account.password;
    if pass.is_empty() || pass.len() > MAX_PASSWORD || !pass.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(AccountError::InvalidPassword);
    }
    if account.gm_level > MAX_GM_LEVEL {
        return Err(AccountError::InvalidGmLevel);
    }
    Ok((name, pass.to_ascii_uppercase()))
}

/// The statements of `AccountMgr::CreateAccount` (+ `account_access` for a
/// GM level), in one transaction; prints the new account id. Only the
/// checked name and the hex hash reach SQL literals, never the password.
fn account_sql(auth: &str, username: &str, hash: &str, gm_level: u8) -> String {
    let mut sql = String::new();
    let _ = writeln!(sql, "USE `{auth}`;");
    sql.push_str("START TRANSACTION;\n");
    let _ = writeln!(
        sql,
        "INSERT INTO account (username, sha_pass_hash, email, reg_mail, joindate) \
         VALUES ('{username}', '{hash}', '', '', NOW());"
    );
    sql.push_str("SET @id = LAST_INSERT_ID();\n");
    sql.push_str(
        "INSERT INTO realmcharacters (realmid, acctid, numchars) \
         SELECT DISTINCT realmlist.id, account.id, 0 FROM realmlist, account \
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

/// Replaces the account's GM level on every realm (`-1`).
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

/// `ChrRaces.dbc` / `ChrClasses.dbc` ids of 5.4.8.
const RACES: &[(&str, u8)] = &[
    ("human", 1),
    ("orc", 2),
    ("dwarf", 3),
    ("night-elf", 4),
    ("undead", 5),
    ("tauren", 6),
    ("gnome", 7),
    ("troll", 8),
    ("goblin", 9),
    ("blood-elf", 10),
    ("draenei", 11),
    ("worgen", 22),
    ("pandaren", 24),
    ("pandaren", 25),
    ("pandaren", 26),
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
    ("monk", 10),
    ("druid", 11),
];

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

/// Realm type shown in the client's realm list: worldserver's `GameType`
/// (0 normal, 1 PvP, 4 normal, 6 RP, 8 RP-PvP, 16 FFA PvP), copied to
/// `realmlist.icon` before each start. Unknown or unreadable: 0.
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

/// Realm 1 at the current address (`flag` has no default in this schema;
/// worldserver keeps it up to date once running).
fn realm_sql(auth: &str, address: std::net::Ipv4Addr, port: u16, icon: u8) -> String {
    format!(
        "INSERT INTO `{auth}`.realmlist (id, name, address, localAddress, port, icon, flag, gamebuild) \
         VALUES ({REALM_ID}, 'Pandaria', '{address}', '{address}', {port}, {icon}, 0, {GAME_BUILD}) AS new \
         ON DUPLICATE KEY UPDATE address = new.address, localAddress = new.localAddress, \
         port = new.port, icon = new.icon, gamebuild = new.gamebuild;\n"
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
    fn the_password_hash_is_the_upstream_one() {
        // `AccountMgr::CalculateShaPassHash("ADMIN", "ADMIN")`, as stored by
        // `account create admin admin` (upper-case hex).
        assert_eq!(
            sha_pass_hash("ADMIN", "ADMIN"),
            "8301316D0D8448A34FA6D0C6BF1CBFA2B4A1A93A"
        );
        let (name, pass) = normalize(&account("Player", "secret1", 0)).unwrap();
        assert_eq!(
            sha_pass_hash(&name, &pass),
            "6E56718D1EC18A524B787C98B0831224F880713D"
        );
    }

    #[test]
    fn names_and_passwords_are_checked() {
        assert!(matches!(
            normalize(&account("", "x", 0)),
            Err(AccountError::InvalidName)
        ));
        assert!(matches!(
            normalize(&account("a'b", "x", 0)),
            Err(AccountError::InvalidName)
        ));
        assert!(matches!(
            normalize(&account(&"a".repeat(17), "x", 0)),
            Err(AccountError::InvalidName)
        ));
        assert!(matches!(
            normalize(&account("ok", "with space", 0)),
            Err(AccountError::InvalidPassword)
        ));
        assert!(matches!(
            normalize(&account("ok", "x", 4)),
            Err(AccountError::InvalidGmLevel)
        ));
    }

    #[test]
    fn a_gm_account_gets_access_on_every_realm() {
        let sql = account_sql("mop_auth", "ADMIN", "AB", 3);
        assert!(sql.contains("VALUES ('ADMIN', 'AB', '', '', NOW())"));
        assert!(sql.contains("VALUES (@id, 3, -1)"));
        assert!(!account_sql("mop_auth", "P", "AB", 0).contains("account_access"));
    }

    #[test]
    fn the_realm_points_at_the_address_with_the_client_build() {
        let sql = realm_sql(
            "mop_auth",
            std::net::Ipv4Addr::new(192, 168, 1, 20),
            8086,
            1,
        );
        assert!(sql.contains("'192.168.1.20', '192.168.1.20', 8086, 1, 0, 18414"));
    }

    #[test]
    fn population_and_online_lines_are_parsed() {
        assert_eq!(
            parse_population("2\t480\n").unwrap(),
            Population {
                players: 2,
                bots: 480
            }
        );
        let online = parse_online("Lia\t90\t24\t10\t0\nBot\t12\t9\t3\t1\n").unwrap();
        assert_eq!(online[0].race, "pandaren");
        assert_eq!(online[0].class, "monk");
        assert!(online[1].bot);
        assert_eq!(online[1].race, "goblin");
    }
}

/// Writes the statements of the driver for the given auth database name
/// into `RK_PANDARIA_SQL_OUT` (a directory), to replay them by hand against
/// a real LoA Pandaria schema. Skipped when the variable is unset.
#[cfg(test)]
#[test]
fn write_sql_for_a_manual_check() {
    let Ok(dir) = std::env::var("RK_PANDARIA_SQL_OUT") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    let (name, pass) = normalize(&NewAccount {
        username: "rkcheck".into(),
        password: "secret1".into(),
        gm_level: 3,
    })
    .unwrap();
    let files = [
        (
            "account.sql",
            account_sql("auth", &name, &sha_pass_hash(&name, &pass), 3),
        ),
        (
            "realm.sql",
            realm_sql("auth", std::net::Ipv4Addr::new(127, 0, 0, 1), 8085, 0),
        ),
        ("accounts.sql", accounts_sql("auth")),
        ("gm.sql", gm_level_sql("auth", 1, 2)),
        ("population.sql", population_sql("auth", "characters")),
        ("online.sql", online_sql("auth", "characters", 10)),
    ];
    for (file, sql) in files {
        std::fs::write(dir.join(file), sql).unwrap();
    }
}

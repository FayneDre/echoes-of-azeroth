//! AzerothCore driver (ADR 0004): what the manifest cannot declare.
//!
//! - **Accounts** are written to the auth database directly, exactly as
//!   `AccountMgr::CreateAccount` does (SRP6 salt + verifier, upper-cased
//!   name and password, `realmcharacters` rows, `account_access` for GM
//!   levels). No running worldserver is needed.
//! - **Realm list:** before each start, realm 1 (`RealmID` in
//!   `worldserver.conf`) points at the current address and world port, so a
//!   changed LAN address never strands the players.

#![forbid(unsafe_code)]

pub mod srp6;

use std::fmt::Write as _;

use eoa_runtime::{DriverContext, DriverFuture, EmulatorDriver, NewAccount};
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
    fn account_sql_grants_gm_level_only_when_asked() {
        let sql = account_sql("acore_auth", "P", &[0xab; 32], &[0xcd; 32], 0);
        assert!(sql.contains(&format!("X'{}'", "ab".repeat(32))));
        assert!(!sql.contains("account_access"));
        let sql = account_sql("acore_auth", "P", &[0; 32], &[0; 32], 3);
        assert!(sql.contains("VALUES (@id, 3, -1)"));
    }
}

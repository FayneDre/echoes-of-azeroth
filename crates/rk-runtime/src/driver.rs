//! What an emulator driver implements (ADR 0004): only what cannot be
//! declared in the manifest.

use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;

use rk_core::manifest::EmulatorManifest;
use rk_engine::install::InstanceRecord;

/// Errors stay typed so the UI can translate them (principle 8).
pub type DriverError = Box<dyn std::error::Error + Send + Sync>;
pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, DriverError>> + Send + 'a>>;

/// What a driver works with. The database is running.
pub struct DriverContext<'a> {
    pub db: &'a rk_db::Instance,
    pub emulator: &'a EmulatorManifest,
    pub record: &'a InstanceRecord,
    /// Address the game client connects to: `127.0.0.1` in solo, this
    /// computer's LAN address in LAN mode.
    pub realm_address: Ipv4Addr,
    /// The instance's rendered config files (`instances/<id>/configs`).
    pub configs: &'a std::path::Path,
}

impl DriverContext<'_> {
    /// Name of a database declared in the emulator manifest.
    pub fn database(&self, id: &str) -> Option<&str> {
        self.emulator
            .databases
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.as_str())
    }

    /// Port declared in the manifest, as allocated to this instance.
    pub fn port(&self, name: &str) -> Option<u16> {
        self.record.ports.get(name).copied()
    }
}

/// A game account to create.
#[derive(Debug, Clone)]
pub struct NewAccount {
    pub username: String,
    pub password: String,
    /// 0 = player; higher levels are emulator-defined (AzerothCore: 3 = admin).
    pub gm_level: u8,
}

/// A character to create without a client (e.g. the AHBot seller).
#[derive(Debug, Clone)]
pub struct NewCharacter {
    pub account_id: u32,
    pub name: String,
    /// Emulator-defined names (`human`, `warrior`...); `None` = driver default.
    pub race: Option<String>,
    pub class: Option<String>,
}

/// Characters in the world right now (live realm state on the Home page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Population {
    /// Characters of player accounts.
    pub players: u32,
    /// Characters of bot accounts.
    pub bots: u32,
}

/// A player account (bot accounts are left out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountInfo {
    pub id: u32,
    pub username: String,
    /// 0 = player; see [`NewAccount::gm_level`].
    pub gm_level: u8,
}

/// A character in the world right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlineCharacter {
    pub name: String,
    pub level: u8,
    /// Emulator-defined names (`human`, `warrior`...), as in [`NewCharacter`].
    pub race: String,
    pub class: String,
    /// Played by a bot account.
    pub bot: bool,
}

pub trait EmulatorDriver: Send + Sync {
    /// Id matched against the emulator manifest's `driver` field.
    fn id(&self) -> &str;

    /// Runs after the database starts and before the servers: e.g. points the
    /// realm list at the current address (a LAN address can change).
    fn prepare_start<'a>(&'a self, cx: &'a DriverContext<'a>) -> DriverFuture<'a, ()>;

    /// Creates a game account; returns its id.
    fn create_account<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        account: &'a NewAccount,
    ) -> DriverFuture<'a, u32>;

    /// Id of an existing account (resuming an interrupted post-install).
    fn find_account<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        username: &'a str,
    ) -> DriverFuture<'a, Option<u32>>;

    /// Creates a character directly in the database; returns its guid. The
    /// world server must be stopped (it allocates guids at startup).
    fn create_character<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        character: &'a NewCharacter,
    ) -> DriverFuture<'a, u32>;

    /// `(account id, guid)` of an existing character.
    fn find_character<'a>(
        &'a self,
        cx: &'a DriverContext<'a>,
        name: &'a str,
    ) -> DriverFuture<'a, Option<(u32, u32)>>;

    /// Characters in the world while the servers run; `None` when the
    /// emulator cannot tell.
    fn population<'a>(
        &'a self,
        _cx: &'a DriverContext<'a>,
    ) -> DriverFuture<'a, Option<Population>> {
        Box::pin(async { Ok(None) })
    }

    /// Player accounts, by name; `None` when the emulator cannot tell.
    fn accounts<'a>(
        &'a self,
        _cx: &'a DriverContext<'a>,
    ) -> DriverFuture<'a, Option<Vec<AccountInfo>>> {
        Box::pin(async { Ok(None) })
    }

    /// Sets the GM level of an existing account (applies at its next login).
    fn set_gm_level<'a>(
        &'a self,
        _cx: &'a DriverContext<'a>,
        _username: &'a str,
        _gm_level: u8,
    ) -> DriverFuture<'a, ()> {
        Box::pin(async { Err("this emulator cannot change GM levels".into()) })
    }

    /// Characters in the world, players first then bots, at most `limit`.
    fn online_characters<'a>(
        &'a self,
        _cx: &'a DriverContext<'a>,
        _limit: u32,
    ) -> DriverFuture<'a, Option<Vec<OnlineCharacter>>> {
        Box::pin(async { Ok(None) })
    }

    /// Named places a character can be teleported to (GM panel).
    fn teleport_locations<'a>(
        &'a self,
        _cx: &'a DriverContext<'a>,
    ) -> DriverFuture<'a, Vec<String>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

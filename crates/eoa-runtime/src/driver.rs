//! What an emulator driver implements (ADR 0004): only what cannot be
//! declared in the manifest.

use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;

use eoa_core::manifest::EmulatorManifest;
use eoa_engine::install::InstanceRecord;

/// Errors stay typed so the UI can translate them (principle 8).
pub type DriverError = Box<dyn std::error::Error + Send + Sync>;
pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, DriverError>> + Send + 'a>>;

/// What a driver works with. The database is running.
pub struct DriverContext<'a> {
    pub db: &'a eoa_db::Instance,
    pub emulator: &'a EmulatorManifest,
    pub record: &'a InstanceRecord,
    /// Address the game client connects to: `127.0.0.1` in solo, this
    /// computer's LAN address in LAN mode.
    pub realm_address: Ipv4Addr,
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
}

//! Server supervisor (architecture §10, ADR 0011).
//!
//! [`Server`] starts an installed instance: its MySQL database, the driver's
//! preparation (realm address), then the emulator's processes in manifest
//! order, each gated by its readiness (TCP port or output line). [`Running`]
//! gives the console, the output and crash detection, and stops everything in
//! reverse order.
//!
//! - **Working directory** = instance root (servers read `configs/` from it).
//! - **Output** of each process goes to `logs/<id>.console.log` (previous run
//!   kept as `.previous.log`) and to a broadcast for the UI / CLI.
//! - **Stop methods** come from the manifest: `close-stdin` (the process saves
//!   and exits at end of input; worldserver) or `terminate` (stateless;
//!   authserver). A `close-stdin` process also stops cleanly when the launcher
//!   dies, as its stdin pipe closes; a `terminate` one sits in a kill-on-close
//!   Job Object so it never outlives the launcher.
//! - **MySQL** is left running if the launcher dies (a world shutdown still
//!   needs it to save); the next start or stop adopts it (`rk-db` pings).

#![forbid(unsafe_code)]

mod driver;
mod error;
pub mod modules;
mod network;
mod process;
mod server;

pub use driver::{
    AccountInfo, DriverContext, DriverError, DriverFuture, EmulatorDriver, NewAccount,
    NewCharacter, OnlineCharacter, Population,
};
pub use error::RuntimeError;
pub use network::{lan_ipv4, port_owner};
pub use process::{Exit, LogLine, Process, ProcessConfig, Ready, Stopped, Stream};
pub use server::{Observer, Running, Server, ServerEvent};

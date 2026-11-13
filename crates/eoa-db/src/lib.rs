//! Portable MySQL management for one realm instance (ADR 0002).
//!
//! One [`Instance`] = one private MySQL server (own data directory, own port,
//! bound to `127.0.0.1`), run from the unpacked official ZIP, without a Windows
//! service or admin rights:
//!
//! ```text
//! <root>/my.ini        server options (rendered by `settings`)
//! <root>/client.cnf    root credentials for the MySQL tools (never on a command line)
//! <root>/data/         data directory
//! <root>/mysqld.log    server error log (shown in support bundles)
//! ```
//!
//! - **Secure by default:** root gets a random password at initialisation
//!   (`--init-file`, so it never listens without one); service accounts get
//!   their own random passwords.
//! - **Clean shutdown only:** [`Instance::stop`] uses `mysqladmin shutdown` and
//!   waits; the process is never killed while healthy.
//! - **Backups:** consistent `mysqldump --single-transaction` dumps, written
//!   atomically; restore replays them.

#![forbid(unsafe_code)]

mod error;
mod instance;
mod secrets;
mod settings;

pub use error::DbError;
pub use instance::{Binaries, Instance};
pub use secrets::{generate_password, validate_identifier};
pub use settings::ServerSettings;

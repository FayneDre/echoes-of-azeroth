//! Domain model of Echoes of Azeroth.
//!
//! This crate is pure: no filesystem, network or process access. It defines
//! the manifest types (mirroring `catalog/schema/*.schema.json`), the
//! in-memory [`Catalog`], and the module [`resolve`] algorithm that turns a
//! user selection into a deterministic installation set.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod manifest;
pub mod resolve;
pub mod text;

pub use catalog::Catalog;
pub use resolve::{resolve, Resolution, ResolveError};
pub use text::LocalizedText;

/// Manifest format version supported by this build of the launcher.
pub const SCHEMA_VERSION: u32 = 1;

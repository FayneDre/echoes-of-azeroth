//! Catalog loading and semantic validation.
//!
//! Layout (see `catalog/README.md`):
//! ```text
//! catalog/
//!   clients/<id>.json
//!   components/<id>.json
//!   emulators/<emulator>/emulator.json
//!   emulators/<emulator>/flavors/<id>.json
//!   emulators/<emulator>/modules/<id>.json
//! ```
//! JSON Schema validation is done by `tools/validate-catalog.mjs`; this crate
//! checks what a schema cannot: references between manifests.
//!
//! [`index`]: the signed artifact index (ADR 0010).

#![forbid(unsafe_code)]

pub mod index;
mod load;
mod validate;

pub use load::{load_dir, LoadError};
pub use validate::{validate, Issue};

//! `.conf` handling for TrinityCore-family emulators (AzerothCore, TrinityCore...).
//!
//! The format is line based: `Key = value`, `#` comments, values either bare
//! (`1`, `3.5`) or double-quoted strings. This crate is pure (no I/O):
//!
//! - [`ConfFile`] parses **losslessly** (comments, order and spacing are kept)
//!   so upstream documentation stays readable in generated files.
//! - [`render`] regenerates an effective file from the upstream `.conf.dist`
//!   plus ordered [`Layer`]s, and reports the [`Provenance`] of every managed key
//!   (the UI shows *why* a value is set or locked).
//! - [`manual_changes`] finds keys the user edited by hand since the last render,
//!   so they are imported as user overrides instead of being overwritten.

#![forbid(unsafe_code)]

mod file;
mod render;

pub use file::{ConfError, ConfFile};
pub use render::{manual_changes, render, Layer, LayerKind, Provenance, Rendered};

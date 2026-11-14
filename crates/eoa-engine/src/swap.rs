//! Atomic replacement of a directory, reversible until the operation ends.
//!
//! For directories the launcher owns entirely and can rebuild (`bin/`, `sql/`,
//! managed overlays), never for player data (databases are migrated and backed
//! up, principle 5). Every sibling path lives next to the target, so renames
//! stay on one volume and are atomic:
//!
//! ```text
//! <name>.eoa-staging/    being filled (incomplete until sealed)
//! <name>.eoa-ready/      sealed: the commit will put it in place
//! <name>.eoa-previous/   version before the operation (kept for undo)
//! <name>.eoa-created     marker: the target did not exist before the operation
//! <name>.eoa-owner       run id of the execution that the two above belong to
//! <name>.eoa-trash/      being deleted
//! ```
//!
//! Typical step: `run` = [`DirSwap::prepare`] + fill the staging directory +
//! [`DirSwap::commit`]; `undo` = [`DirSwap::undo`]; `finalize` =
//! [`DirSwap::finalize`]. A crash at any point is repaired by the next call
//! (each one starts with [`DirSwap::recover`]).
//!
//! The owner marker ties rollback data to one execution
//! ([`StepContext::run_id`](crate::StepContext::run_id)): leftovers of another
//! one (whose cleanup failed) are settled, never restored, so an undo can only
//! bring back the version from before *this* execution.
//!
//! Renames fail on Windows while a file inside is open: stop the servers first.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{fsutil, EngineError};

#[derive(Debug, Clone)]
pub struct DirSwap {
    target: PathBuf,
    name: OsString,
    run: String,
}

impl DirSwap {
    /// `run` identifies the execution (use [`StepContext::run_id`](crate::StepContext::run_id)).
    pub fn new(target: impl Into<PathBuf>, run: impl Into<String>) -> Result<Self, EngineError> {
        let target = target.into();
        let run = run.into();
        let name = match (target.file_name(), target.parent()) {
            (Some(name), Some(_)) if !run.is_empty() => name.to_owned(),
            _ => {
                return Err(EngineError::InvalidOperation(format!(
                    "cannot swap '{}': not a named directory, or no run id",
                    target.display()
                )))
            }
        };
        Ok(Self { target, name, run })
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut name = self.name.clone();
        name.push(suffix);
        self.target.with_file_name(name)
    }

    pub fn staging(&self) -> PathBuf {
        self.sibling(".eoa-staging")
    }
    fn ready(&self) -> PathBuf {
        self.sibling(".eoa-ready")
    }
    fn previous(&self) -> PathBuf {
        self.sibling(".eoa-previous")
    }
    fn created(&self) -> PathBuf {
        self.sibling(".eoa-created")
    }
    fn owner(&self) -> PathBuf {
        self.sibling(".eoa-owner")
    }
    fn trash(&self) -> PathBuf {
        self.sibling(".eoa-trash")
    }

    /// The rollback data next to the target belongs to this execution.
    fn owned(&self) -> bool {
        std::fs::read_to_string(self.owner()).is_ok_and(|owner| owner == self.run)
    }

    /// Returns an empty staging directory to fill.
    pub fn prepare(&self) -> Result<PathBuf, EngineError> {
        self.recover()?;
        let staging = self.staging();
        fsutil::remove_dir_all(&staging)?;
        std::fs::create_dir_all(&staging).map_err(EngineError::io(&staging))?;
        Ok(staging)
    }

    /// Puts the filled staging directory in place, keeping the version from
    /// before this execution. Idempotent across a resumed step.
    pub fn commit(&self) -> Result<(), EngineError> {
        self.recover()?;
        let staging = self.staging();
        if !staging.is_dir() {
            return Err(EngineError::io(&staging)(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "nothing staged to commit",
            )));
        }
        if !self.owned() {
            fsutil::write_atomic(&self.owner(), self.run.as_bytes())?;
        }
        // Sealing is one atomic rename: from now on the commit is finished
        // forward by `recover`, even after a crash.
        fsutil::rename(&staging, &self.ready())?;
        self.finish_commit()
    }

    fn finish_commit(&self) -> Result<(), EngineError> {
        let (previous, created) = (self.previous(), self.created());
        if self.target.exists() {
            if previous.exists() || created.exists() {
                // Committed already in this execution: the older state is kept.
                fsutil::rename(&self.target, &self.trash())?;
            } else {
                fsutil::rename(&self.target, &previous)?;
            }
        } else if !previous.exists() {
            fsutil::write_atomic(&created, b"")?;
        }
        fsutil::rename(&self.ready(), &self.target)?;
        fsutil::remove_dir_all(&self.trash())
    }

    /// Finishes a commit interrupted by a crash, removes trash and settles
    /// leftovers of another execution.
    pub fn recover(&self) -> Result<(), EngineError> {
        if self.ready().is_dir() {
            self.finish_commit()?;
        }
        fsutil::remove_dir_all(&self.trash())?;
        if !self.owned() {
            self.settle()?;
        }
        Ok(())
    }

    /// Restores the state from before this execution: the previous version, or
    /// no directory at all if it was created. Nothing happens if it committed
    /// nothing.
    pub fn undo(&self) -> Result<(), EngineError> {
        self.recover()?;
        fsutil::remove_dir_all(&self.staging())?;
        if !self.owned() {
            return Ok(());
        }
        let previous = self.previous();
        if previous.is_dir() {
            if self.target.exists() {
                fsutil::rename(&self.target, &self.trash())?;
            }
            fsutil::rename(&previous, &self.target)?;
            fsutil::remove_dir_all(&self.trash())?;
        } else if self.created().exists() && self.target.exists() {
            fsutil::rename(&self.target, &self.trash())?;
            fsutil::remove_dir_all(&self.trash())?;
        }
        fsutil::remove_file(&self.created())?;
        fsutil::remove_file(&self.owner())
    }

    /// Drops what was kept for an undo; the new version becomes permanent.
    pub fn finalize(&self) -> Result<(), EngineError> {
        self.recover()?;
        fsutil::remove_dir_all(&self.staging())?;
        self.settle()
    }

    /// Removes rollback data, restoring the previous version first if the
    /// target is missing (an undo interrupted by a crash): never loses both.
    fn settle(&self) -> Result<(), EngineError> {
        let previous = self.previous();
        if previous.is_dir() {
            if self.target.exists() {
                fsutil::remove_dir_all(&previous)?;
            } else {
                fsutil::rename(&previous, &self.target)?;
            }
        }
        fsutil::remove_file(&self.created())?;
        fsutil::remove_file(&self.owner())
    }
}

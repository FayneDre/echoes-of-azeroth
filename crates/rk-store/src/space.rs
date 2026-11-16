//! Free disk space checks: a full disk stops a task before it starts, with
//! the figures the player needs, rather than halfway with an I/O error.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Kept free on top of what a task needs (journals, logs, temporary files).
pub const MARGIN: u64 = 256 * 1024 * 1024;

/// Not enough free space on the disk holding `path`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "not enough free space on the disk of {}: {needed} bytes needed, {available} available",
    path.display()
)]
pub struct NoSpace {
    pub path: PathBuf,
    /// Bytes needed, margin included.
    pub needed: u64,
    pub available: u64,
}

/// Bytes available to this user on the disk holding `path` (the path itself
/// may not exist yet: its nearest existing ancestor is measured).
pub fn available_space(path: &Path) -> std::io::Result<u64> {
    let absolute = std::path::absolute(path)?;
    let mut probe = absolute.as_path();
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => break,
        }
    }
    fs4::available_space(probe)
}

/// Fails when the disk of `path` has less than `needed` bytes (plus
/// [`MARGIN`]) free. A disk whose free space cannot be read never blocks:
/// the task itself then reports a full disk.
pub fn ensure_space(path: &Path, needed: u64) -> Result<(), NoSpace> {
    let Ok(available) = available_space(path) else {
        return Ok(());
    };
    let needed = needed.saturating_add(MARGIN);
    if available < needed {
        Err(NoSpace {
            path: path.to_path_buf(),
            needed,
            available,
        })
    } else {
        Ok(())
    }
}

/// Whether an I/O error means the disk is full.
pub fn is_disk_full(error: &std::io::Error) -> bool {
    // ERROR_HANDLE_DISK_FULL (39), ERROR_DISK_FULL (112).
    error.kind() == std::io::ErrorKind::StorageFull
        || (cfg!(windows) && matches!(error.raw_os_error(), Some(39 | 112)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_is_measured_on_its_disk() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not/yet/created");
        assert!(available_space(&missing).unwrap() > 0);
        assert!(ensure_space(&missing, 0).is_ok() || available_space(dir.path()).unwrap() < MARGIN);
    }

    #[test]
    fn an_impossible_need_is_refused_with_the_figures() {
        let dir = tempfile::tempdir().unwrap();
        let error = ensure_space(dir.path(), u64::MAX / 2).unwrap_err();
        assert_eq!(error.needed, u64::MAX / 2 + MARGIN);
        assert!(error.available < error.needed);
    }
}

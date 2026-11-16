//! Filesystem primitives with short retries for transient Windows errors
//! (antivirus or indexer briefly holding a file just written).

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use crate::EngineError;

const ATTEMPTS: u32 = 8;

fn is_transient(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::PermissionDenied {
        return true;
    }
    // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
    cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33))
}

/// Retries `f` on transient errors (blocking, at most ~1 s in total). Only used
/// for small, rare operations: journal writes and directory renames.
fn retry<T>(mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut attempt = 1;
    loop {
        match f() {
            Err(e) if attempt < ATTEMPTS && is_transient(&e) => {
                std::thread::sleep(Duration::from_millis(25 * u64::from(attempt)));
                attempt += 1;
            }
            result => return result,
        }
    }
}

pub(crate) fn rename(from: &Path, to: &Path) -> Result<(), EngineError> {
    retry(|| std::fs::rename(from, to)).map_err(EngineError::io(from))
}

/// Removes a directory tree; a missing one is not an error.
pub(crate) fn remove_dir_all(path: &Path) -> Result<(), EngineError> {
    match retry(|| std::fs::remove_dir_all(path)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(EngineError::io(path)(e)),
        _ => Ok(()),
    }
}

/// Removes a file; a missing one is not an error.
pub(crate) fn remove_file(path: &Path) -> Result<(), EngineError> {
    match retry(|| std::fs::remove_file(path)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(EngineError::io(path)(e)),
        _ => Ok(()),
    }
}

/// Writes a file durably and atomically (temp file + fsync + rename): readers
/// see either the old or the new content, never a partial one.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), EngineError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = Path::new(&tmp);
    let write = || -> io::Result<()> {
        let mut file = std::fs::File::create(tmp)?;
        file.write_all(bytes)?;
        file.sync_all()
    };
    retry(write).map_err(EngineError::io(tmp))?;
    rename(tmp, path)
}

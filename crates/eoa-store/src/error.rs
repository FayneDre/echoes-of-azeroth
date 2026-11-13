use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("invalid sha256 '{0}'")]
    InvalidHash(String),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected HTTP status {status}")]
    Status { status: u16 },
    #[error("server reports size {actual}, expected {expected}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("incomplete transfer: expected {expected} bytes, got {got}")]
    Incomplete { expected: u64, got: u64 },
    #[error("downloaded data has sha256 {actual}, expected {expected}")]
    HashMismatch { expected: String, actual: String },
    #[error("all sources failed:\n{}", .0.join("\n"))]
    AllSourcesFailed(Vec<String>),
}

impl StoreError {
    /// Errors a retry on the same source cannot fix (wrong size, client errors).
    pub(crate) fn is_permanent(&self) -> bool {
        match self {
            StoreError::SizeMismatch { .. } => true,
            StoreError::Status { status } => {
                (400..500).contains(status) && *status != 408 && *status != 429
            }
            _ => false,
        }
    }

    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> StoreError + '_ {
        move |source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

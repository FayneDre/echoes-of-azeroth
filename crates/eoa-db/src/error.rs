use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors carry what the UI needs for an actionable message (port, log excerpt).
#[derive(Debug, Error)]
pub enum DbError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("MySQL program not found: {0}")]
    MissingBinary(PathBuf),
    #[error("port {port} on 127.0.0.1 is already in use by another program")]
    PortInUse { port: u16 },
    #[error("the data directory {0} already contains a database")]
    AlreadyInitialized(PathBuf),
    #[error("the data directory {0} has not been initialised")]
    NotInitialized(PathBuf),
    #[error("MySQL initialisation failed:\n{log}")]
    InitFailed { log: String },
    #[error("MySQL stopped during startup (exit code {code:?}):\n{log}")]
    ExitedDuringStartup { code: Option<i32>, log: String },
    #[error("MySQL did not become ready within {seconds} s:\n{log}")]
    StartTimeout { seconds: u64, log: String },
    #[error("MySQL did not stop within {seconds} s")]
    StopTimeout { seconds: u64 },
    #[error("{tool} failed ({status}): {stderr}")]
    Tool {
        tool: &'static str,
        status: String,
        stderr: String,
    },
    #[error("invalid identifier '{0}': use letters, digits and '_' only")]
    InvalidIdentifier(String),
    #[error("could not generate a random password: {0}")]
    Random(String),
}

impl DbError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> DbError + '_ {
        move |source| DbError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

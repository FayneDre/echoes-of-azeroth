use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors carry what the UI needs for an actionable message (process, port,
/// last lines of output).
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("program not found: {0}")]
    MissingProgram(PathBuf),
    #[error("{process}: port {port} is already in use by another program")]
    PortInUse { process: String, port: u16 },
    #[error("{process} stopped during startup (exit code {code:?}):\n{output}")]
    ExitedDuringStartup {
        process: String,
        code: Option<i32>,
        output: String,
    },
    #[error("{process} did not become ready within {seconds} s:\n{output}")]
    StartTimeout {
        process: String,
        seconds: u64,
        output: String,
    },
    #[error("{process} is not running")]
    NotRunning { process: String },
    #[error("invalid readiness pattern for {process}: {message}")]
    Pattern { process: String, message: String },
    #[error("unknown port '{port}' in the readiness of {process}")]
    UnknownPort { process: String, port: String },
    #[error("could not attach {process} to a job object: {message}")]
    Job { process: String, message: String },
    #[error(transparent)]
    Db(#[from] eoa_db::DbError),
    #[error(transparent)]
    Plan(#[from] eoa_engine::install::PlanError),
    #[error("the {driver} driver failed")]
    Driver {
        driver: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl RuntimeError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> RuntimeError + '_ {
        move |source| RuntimeError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::StepError;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid operation: {0}")]
    InvalidOperation(String),
    #[error("operation '{0}' is already running")]
    Busy(String),
    #[error(
        "operation '{operation}' was interrupted with a different plan; \
         finish or abort it with its original plan first"
    )]
    PlanChanged { operation: String },
    #[error("operation '{operation}' needs repair: {failure}")]
    NeedsRepair { operation: String, failure: String },
    #[error("operation '{0}' is already complete and cannot be aborted")]
    AlreadyCompleted(String),
    #[error("step '{step}' failed: {source}{}", rollback_suffix(.rollback))]
    StepFailed {
        step: String,
        #[source]
        source: StepError,
        /// `None` when every started step was undone.
        rollback: Option<RollbackFailure>,
    },
    #[error("rollback failed at step '{}': {}", .0.step, .0.source)]
    RollbackFailed(RollbackFailure),
    #[error("journal {path} is unreadable: {message}")]
    CorruptJournal { path: PathBuf, message: String },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The step whose `undo` failed, leaving the operation to repair.
#[derive(Debug)]
pub struct RollbackFailure {
    pub step: String,
    pub source: StepError,
}

fn rollback_suffix(rollback: &Option<RollbackFailure>) -> String {
    match rollback {
        None => String::new(),
        Some(f) => format!(" (rollback failed at step '{}': {})", f.step, f.source),
    }
}

impl EngineError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> EngineError + '_ {
        move |source| EngineError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use thiserror::Error;

/// Error returned by a step. Boxed so steps keep their typed errors
/// (`StoreError`, `DbError`...) for the UI to translate, through `downcast_ref`.
pub type StepError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Future returned by [`Step`] methods.
pub type StepFuture<'a> = BoxFuture<'a, Result<(), StepError>>;

/// One unit of work of an operation.
///
/// Contract:
/// - `run` is **idempotent**: it may be called again after an interruption at
///   any point ([`StepContext::is_resumed`] tells it so).
/// - `undo` reverts what `run` did, tolerates a partial or absent `run`, and is
///   idempotent too. Steps that leave nothing to revert (a download into the
///   content-addressed store) keep the default no-op.
/// - `finalize` drops what was kept only for a rollback (previous versions);
///   it runs once every step is done and must be idempotent.
pub trait Step<C>: Send + Sync {
    /// Unique and stable within the operation (e.g. `extract-server`).
    fn id(&self) -> &str;

    /// Step type from the closed vocabulary (e.g. `extract`), for logs and the UI.
    fn kind(&self) -> &str;

    /// Parameters that define what the step does (versions, hashes, paths).
    /// Part of the plan fingerprint: an interrupted operation only resumes
    /// with the very same plan.
    fn fingerprint(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a>;

    fn undo<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn finalize<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

/// What a step receives: the caller's context plus engine services.
pub struct StepContext<'a, C> {
    pub(crate) context: &'a C,
    pub(crate) operation: &'a str,
    pub(crate) step: &'a str,
    pub(crate) run: &'a str,
    pub(crate) resumed: bool,
    pub(crate) observer: &'a dyn Observer,
    pub(crate) cancel: &'a CancelToken,
}

impl<C> StepContext<'_, C> {
    /// The caller's context (store, paths, database handles...).
    pub fn context(&self) -> &C {
        self.context
    }

    pub fn operation(&self) -> &str {
        self.operation
    }

    /// Id of this execution of the plan, stable across resumes and rollback
    /// (e.g. for [`DirSwap::new`](crate::DirSwap::new)).
    pub fn run_id(&self) -> &str {
        self.run
    }

    /// `true` when this step had already started in an interrupted run.
    pub fn is_resumed(&self) -> bool {
        self.resumed
    }

    /// Reports progress within the step (bytes, files...).
    pub fn progress(&self, done: u64, total: Option<u64>) {
        self.observer.event(&Event::StepProgress {
            step: self.step,
            done,
            total,
        });
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// A handle for work running outside the step's future (blocking threads).
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Returns [`Cancelled`] once cancellation was requested. A step returning
    /// it is interrupted, not failed: no rollback, it resumes on the next run.
    pub fn check_cancelled(&self) -> Result<(), StepError> {
        if self.is_cancelled() {
            Err(Box::new(Cancelled))
        } else {
            Ok(())
        }
    }
}

/// Error a step returns to stop at a safe point after a cancellation request.
#[derive(Debug, Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Cooperative cancellation, checked between steps and by long steps.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Progress notifications, e.g. forwarded to the UI as Tauri events.
#[derive(Debug, Clone, Copy)]
pub enum Event<'a> {
    StepStarted {
        index: usize,
        total: usize,
        step: &'a str,
        kind: &'a str,
        resumed: bool,
    },
    /// Already done in an earlier, interrupted run.
    StepSkipped {
        index: usize,
        total: usize,
        step: &'a str,
    },
    StepFinished {
        index: usize,
        total: usize,
        step: &'a str,
    },
    StepProgress {
        step: &'a str,
        done: u64,
        total: Option<u64>,
    },
    /// Undoing started steps; `reason` is why (failure or abort).
    RollbackStarted {
        reason: &'a str,
    },
    StepUndone {
        step: &'a str,
    },
    /// A cleanup failed; the operation is still complete.
    FinalizeFailed {
        step: &'a str,
        error: &'a (dyn std::error::Error + Send + Sync + 'static),
    },
}

pub trait Observer: Send + Sync {
    fn event(&self, event: &Event<'_>);
}

/// Ignores events.
impl Observer for () {
    fn event(&self, _event: &Event<'_>) {}
}

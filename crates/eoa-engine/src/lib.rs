//! Transactional step executor (architecture §5, principle 4).
//!
//! An [`Operation`] is an ordered list of [`Step`]s (install, update, module
//! toggle...). The [`Engine`] runs it so that it can always be finished or
//! undone, whatever happens to the process:
//!
//! ```text
//! <state>/<operation>.journal.json   progress of the operation (atomic writes)
//! <state>/<operation>.lock           held while the operation runs (OS lock)
//! ```
//!
//! - **Journal:** every state change of a step is persisted *before* the next
//!   action, so a crash, a power cut or a closed launcher leaves an exact record.
//! - **Resume:** running the same operation again skips finished steps and
//!   re-runs the interrupted one (steps are idempotent and are told they resume).
//!   A different plan for an unfinished operation is refused, never mixed.
//! - **Rollback:** when a step fails, the started steps are undone in reverse
//!   order. The rollback is journaled too and resumes after a crash; if it
//!   fails, the operation needs repair and blocks the next ones.
//! - **Finalize:** after the last step, steps drop what they kept for a rollback
//!   (old versions); a failed cleanup never fails the operation.
//! - **Atomic swap:** [`DirSwap`] replaces a directory through a staged copy and
//!   keeps the previous version until the operation is finalized.
//!
//! The engine is generic: what a step does (download, extract, render a config,
//! import SQL...) lives in the step, emulator specifics in drivers.

#![forbid(unsafe_code)]

mod engine;
mod error;
mod fsutil;
mod journal;
mod step;
mod swap;

pub use engine::{Engine, Operation, Outcome};
pub use error::{EngineError, RollbackFailure};
pub use journal::{Journal, Status, StepRecord, StepState};
pub use step::{CancelToken, Cancelled, Event, Observer, Step, StepContext, StepError, StepFuture};
pub use swap::DirSwap;

pub mod install;
pub mod steps;

//! Built-in steps of the closed vocabulary (architecture §5).
//!
//! Each one follows the [`Step`](crate::Step) contract (idempotent `run`,
//! tolerant `undo`) and keeps its typed error for the UI.

mod extract;
mod fetch;

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub use extract::{Extract, ExtractError, Strip};
pub use fetch::Fetch;

use crate::StepContext;

/// Drives `work` while reporting `done` / `total` (0 = unknown) every 250 ms,
/// so a large transfer produces a few events per second, not one per chunk.
/// With `cancellable`, a cancellation request drops `work` and returns `None`
/// (only for work that is safe to drop at any point).
pub(crate) async fn with_progress<C, T>(
    cx: &StepContext<'_, C>,
    done: &AtomicU64,
    total: &AtomicU64,
    cancellable: bool,
    work: impl Future<Output = T>,
) -> Option<T> {
    let report = || {
        let total = total.load(Ordering::Relaxed);
        cx.progress(done.load(Ordering::Relaxed), (total > 0).then_some(total));
    };
    let mut work = std::pin::pin!(work);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            out = &mut work => {
                report();
                return Some(out);
            }
            _ = tick.tick() => {
                report();
                if cancellable && cx.is_cancelled() {
                    return None;
                }
            }
        }
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use eoa_store::{FetchRequest, Progress, Store};

use super::with_progress;
use crate::{Cancelled, Step, StepContext, StepFuture};

/// `download`: brings a verified blob into the content-addressed store.
///
/// Nothing to undo: the store is a cache shared by every instance. Cancelling
/// is safe at any time: the store keeps completed segments and resumes.
pub struct Fetch {
    id: String,
    store: Arc<Store>,
    request: FetchRequest,
}

impl Fetch {
    pub fn new(id: impl Into<String>, store: Arc<Store>, request: FetchRequest) -> Self {
        Self {
            id: id.into(),
            store,
            request,
        }
    }
}

struct Counter<'a>(&'a AtomicU64);

impl Progress for Counter<'_> {
    fn advance(&self, bytes: u64) {
        self.0.fetch_add(bytes, Ordering::Relaxed);
    }
    fn retract(&self, bytes: u64) {
        self.0.fetch_sub(bytes, Ordering::Relaxed);
    }
}

impl<C: Sync> Step<C> for Fetch {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &str {
        "download"
    }

    /// The content, not the mirrors: a changed mirror list is the same plan.
    fn fingerprint(&self) -> String {
        format!(
            "{} {}",
            self.request.sha256.to_ascii_lowercase(),
            self.request.size
        )
    }

    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let done = AtomicU64::new(0);
            let total = AtomicU64::new(self.request.size);
            let counter = Counter(&done);
            let fetch = self.store.fetch(&self.request, &counter);
            match with_progress(cx, &done, &total, true, fetch).await {
                None => Err(Box::new(Cancelled).into()),
                Some(result) => result.map(drop).map_err(Into::into),
            }
        })
    }
}

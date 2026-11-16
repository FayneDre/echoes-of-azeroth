use serde::{Deserialize, Serialize};

/// Journal format version; bumped on incompatible changes.
pub(crate) const FORMAT: u32 = 1;

/// Persisted progress of one operation. Exposed read-only so the UI can show
/// an interrupted operation and offer to resume or abort it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journal {
    pub format: u32,
    pub operation: String,
    /// Hash of the plan (step ids, kinds and parameters).
    pub fingerprint: String,
    /// Unique per execution of the plan (kept across resumes): lets steps tell
    /// their own leftovers from those of an earlier execution of the same id.
    pub run: String,
    pub status: Status,
    pub steps: Vec<StepRecord>,
    /// Why the operation is being, or was, rolled back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// Last change, seconds since the Unix epoch.
    pub updated: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Steps are running (or were, when the process stopped).
    Running,
    /// A step failed or an abort was requested; started steps are being undone.
    RollingBack,
    /// Every step is done; cleanups are running.
    Finalizing,
    Completed,
    RolledBack,
    /// An undo failed: the instance is in an unknown state until repaired.
    RollbackFailed,
}

impl Status {
    /// The operation is over: a new plan may reuse its id.
    pub fn is_terminal(self) -> bool {
        matches!(self, Status::Completed | Status::RolledBack)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepRecord {
    pub id: String,
    pub kind: String,
    pub state: StepState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepState {
    Pending,
    /// Started; when found on resume, the step was interrupted.
    Running,
    Done,
    Failed,
    Undone,
}

impl StepState {
    /// The step may have changed something that a rollback must undo.
    pub(crate) fn needs_undo(self) -> bool {
        matches!(
            self,
            StepState::Running | StepState::Done | StepState::Failed
        )
    }
}

/// A new execution id: time, process and a counter, unique on this machine.
pub(crate) fn new_run_id() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:x}-{:x}-{count:x}", std::process::id())
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

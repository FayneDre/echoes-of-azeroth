//! Engine progress as serializable events for the UI.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eoa_engine::{Event, Observer};
use serde::Serialize;

/// One engine event, owned and serializable.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OperationEvent {
    #[serde(rename_all = "camelCase")]
    StepStarted {
        index: usize,
        total: usize,
        step: String,
        step_kind: String,
        resumed: bool,
    },
    #[serde(rename_all = "camelCase")]
    StepSkipped {
        index: usize,
        total: usize,
        step: String,
    },
    #[serde(rename_all = "camelCase")]
    StepFinished {
        index: usize,
        total: usize,
        step: String,
    },
    #[serde(rename_all = "camelCase")]
    StepProgress {
        step: String,
        done: u64,
        total: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    RollbackStarted { reason: String },
    #[serde(rename_all = "camelCase")]
    StepUndone { step: String },
    #[serde(rename_all = "camelCase")]
    FinalizeFailed { step: String, error: String },
}

impl OperationEvent {
    pub fn from_engine(event: &Event<'_>) -> Option<Self> {
        Some(match event {
            Event::StepStarted {
                index,
                total,
                step,
                kind,
                resumed,
            } => Self::StepStarted {
                index: *index,
                total: *total,
                step: (*step).to_owned(),
                step_kind: (*kind).to_owned(),
                resumed: *resumed,
            },
            Event::StepSkipped { index, total, step } => Self::StepSkipped {
                index: *index,
                total: *total,
                step: (*step).to_owned(),
            },
            Event::StepFinished { index, total, step } => Self::StepFinished {
                index: *index,
                total: *total,
                step: (*step).to_owned(),
            },
            Event::StepProgress { step, done, total } => Self::StepProgress {
                step: (*step).to_owned(),
                done: *done,
                total: *total,
            },
            Event::RollbackStarted { reason } => Self::RollbackStarted {
                reason: (*reason).to_owned(),
            },
            Event::StepUndone { step } => Self::StepUndone {
                step: (*step).to_owned(),
            },
            Event::FinalizeFailed { step, error } => Self::FinalizeFailed {
                step: (*step).to_owned(),
                error: error.to_string(),
            },
            #[allow(unreachable_patterns)]
            _ => return None,
        })
    }
}

/// Forwards engine events to a callback, at most one `StepProgress` per
/// `interval` (a download reports every chunk).
pub struct EventObserver {
    sink: Arc<dyn Fn(OperationEvent) + Send + Sync>,
    interval: Duration,
    last_progress: Mutex<Option<Instant>>,
}

impl EventObserver {
    pub fn new(sink: Arc<dyn Fn(OperationEvent) + Send + Sync>) -> Self {
        Self {
            sink,
            interval: Duration::from_millis(150),
            last_progress: Mutex::new(None),
        }
    }
}

impl Observer for EventObserver {
    fn event(&self, event: &Event<'_>) {
        if let Event::StepProgress { done, total, .. } = event {
            let finished = total.is_some_and(|t| *done >= t);
            let mut last = self.last_progress.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if !finished && last.is_some_and(|at| now.duration_since(at) < self.interval) {
                return;
            }
            *last = Some(now);
        }
        if let Some(event) = OperationEvent::from_engine(event) {
            (self.sink)(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_throttled_but_completion_passes() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let observer = EventObserver::new(Arc::new(move |e| sink.lock().unwrap().push(e)));
        for done in [1, 2, 3] {
            observer.event(&Event::StepProgress {
                step: "download",
                done,
                total: Some(10),
            });
        }
        observer.event(&Event::StepProgress {
            step: "download",
            done: 10,
            total: Some(10),
        });
        let seen = seen.lock().unwrap();
        let dones: Vec<u64> = seen
            .iter()
            .map(|e| match e {
                OperationEvent::StepProgress { done, .. } => *done,
                _ => 0,
            })
            .collect();
        assert_eq!(dones, [1, 10]);
    }

    #[test]
    fn events_serialize_with_a_kind_tag() {
        let event = OperationEvent::StepStarted {
            index: 0,
            total: 3,
            step: "download-server".into(),
            step_kind: "download".into(),
            resumed: false,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "stepStarted");
        assert_eq!(json["stepKind"], "download");
    }
}

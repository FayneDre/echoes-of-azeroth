use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::RollbackFailure;
use crate::journal::{new_run_id, now, FORMAT};
use crate::{
    fsutil, CancelToken, Cancelled, EngineError, Event, Journal, Observer, Status, Step,
    StepContext, StepRecord, StepState,
};

/// An ordered list of steps under one id.
///
/// There is one journal per id: use one id per thing being changed (e.g.
/// `instance-<id>`), so operations on it are serialised and an unfinished one
/// blocks the next until it is resumed or aborted.
pub struct Operation<C> {
    id: String,
    steps: Vec<Box<dyn Step<C>>>,
}

impl<C> Operation<C> {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            steps: Vec::new(),
        }
    }

    pub fn step(mut self, step: impl Step<C> + 'static) -> Self {
        self.steps.push(Box::new(step));
        self
    }

    pub fn push(&mut self, step: Box<dyn Step<C>>) {
        self.steps.push(step);
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    fn validate(&self) -> Result<(), EngineError> {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 100
            && !self.id.starts_with('.')
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !id_ok {
            return Err(EngineError::InvalidOperation(format!(
                "operation id '{}' must be 1-100 ASCII letters, digits, '-', '_' or '.'",
                self.id
            )));
        }
        let mut seen = BTreeSet::new();
        for step in &self.steps {
            if step.id().is_empty() || !seen.insert(step.id()) {
                return Err(EngineError::InvalidOperation(format!(
                    "step id '{}' is empty or duplicated in '{}'",
                    step.id(),
                    self.id
                )));
            }
        }
        Ok(())
    }

    fn fingerprint(&self) -> String {
        // JSON keeps the boundaries between fields unambiguous.
        let plan: Vec<(&str, &str, String)> = self
            .steps
            .iter()
            .map(|s| (s.id(), s.kind(), s.fingerprint()))
            .collect();
        let json = serde_json::to_vec(&(&self.id, plan)).expect("plain strings serialize");
        hex::encode(Sha256::digest(json))
    }

    fn new_journal(&self, fingerprint: String) -> Journal {
        Journal {
            format: FORMAT,
            operation: self.id.clone(),
            fingerprint,
            run: new_run_id(),
            status: Status::Running,
            steps: self
                .steps
                .iter()
                .map(|s| StepRecord {
                    id: s.id().to_owned(),
                    kind: s.kind().to_owned(),
                    state: StepState::Pending,
                })
                .collect(),
            failure: None,
            updated: now(),
        }
    }
}

/// How [`Engine::run`] ended without an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    /// Nothing to do: this exact plan already completed.
    AlreadyCompleted,
    /// Stopped at a safe point; running it again resumes.
    Cancelled,
    /// An earlier run failed and its interrupted rollback has now finished;
    /// `failure` is the original reason.
    RolledBack {
        failure: String,
    },
}

/// Runs operations against a state directory (journals and locks).
pub struct Engine {
    state_dir: PathBuf,
}

impl Engine {
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// The journal of an operation, if any (for the UI: resume or abort?).
    pub fn status(&self, operation: &str) -> Result<Option<Journal>, EngineError> {
        self.load(operation)
    }

    /// Runs, resumes or finishes rolling back `op`.
    ///
    /// A step failure rolls back the started steps and returns
    /// [`EngineError::StepFailed`]. A cancellation keeps the journal so the next
    /// call resumes where it stopped.
    pub async fn run<C: Sync>(
        &self,
        op: &Operation<C>,
        context: &C,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<Outcome, EngineError> {
        op.validate()?;
        let _lock = self.lock(&op.id)?;
        let fingerprint = op.fingerprint();
        let mut journal = match self.load_for(op, &fingerprint)? {
            None => op.new_journal(fingerprint),
            Some(j) if j.status == Status::RollbackFailed => {
                return Err(EngineError::NeedsRepair {
                    operation: op.id.clone(),
                    failure: j.failure.unwrap_or_default(),
                })
            }
            Some(j)
                if j.status.is_terminal()
                    && !(j.status == Status::Completed && j.fingerprint == fingerprint) =>
            {
                op.new_journal(fingerprint)
            }
            Some(j) if j.fingerprint != fingerprint => {
                return Err(EngineError::PlanChanged {
                    operation: op.id.clone(),
                })
            }
            Some(j) => match j.status {
                Status::Completed => return Ok(Outcome::AlreadyCompleted),
                Status::RollingBack => {
                    let mut j = j;
                    let failure = j.failure.clone().unwrap_or_default();
                    observer.event(&Event::RollbackStarted { reason: &failure });
                    return match self.undo_all(op, &mut j, context, observer).await? {
                        Ok(()) => Ok(Outcome::RolledBack { failure }),
                        Err(f) => Err(EngineError::RollbackFailed(f)),
                    };
                }
                Status::Finalizing => {
                    let mut j = j;
                    self.finalize_all(op, &mut j, context, observer).await?;
                    return Ok(Outcome::Completed);
                }
                _ => j,
            },
        };
        self.save(&mut journal)?;

        let run = journal.run.clone();
        let total = op.steps.len();
        for (index, step) in op.steps.iter().enumerate() {
            let state = journal.steps[index].state;
            if state == StepState::Done {
                observer.event(&Event::StepSkipped {
                    index,
                    total,
                    step: step.id(),
                });
                continue;
            }
            if cancel.is_cancelled() {
                return Ok(Outcome::Cancelled);
            }
            let resumed = state == StepState::Running;
            journal.steps[index].state = StepState::Running;
            self.save(&mut journal)?;
            observer.event(&Event::StepStarted {
                index,
                total,
                step: step.id(),
                kind: step.kind(),
                resumed,
            });
            let cx = StepContext {
                context,
                operation: &op.id,
                step: step.id(),
                run: &run,
                resumed,
                observer,
                cancel,
            };
            match step.run(&cx).await {
                Ok(()) => {
                    journal.steps[index].state = StepState::Done;
                    self.save(&mut journal)?;
                    observer.event(&Event::StepFinished {
                        index,
                        total,
                        step: step.id(),
                    });
                }
                // Interrupted at a safe point: stays `Running`, resumes next time.
                Err(e) if e.is::<Cancelled>() => return Ok(Outcome::Cancelled),
                Err(source) => {
                    let failure = format!("step '{}' failed: {source}", step.id());
                    journal.steps[index].state = StepState::Failed;
                    journal.status = Status::RollingBack;
                    journal.failure = Some(failure.clone());
                    self.save(&mut journal)?;
                    observer.event(&Event::RollbackStarted { reason: &failure });
                    let rollback = self
                        .undo_all(op, &mut journal, context, observer)
                        .await?
                        .err();
                    return Err(EngineError::StepFailed {
                        step: step.id().to_owned(),
                        source,
                        rollback,
                    });
                }
            }
        }
        self.finalize_all(op, &mut journal, context, observer)
            .await?;
        Ok(Outcome::Completed)
    }

    /// Abandons an unfinished operation: undoes its started steps. Also retries
    /// a failed rollback (after the cause was fixed). Needs the original plan.
    pub async fn abort<C: Sync>(
        &self,
        op: &Operation<C>,
        context: &C,
        observer: &dyn Observer,
    ) -> Result<(), EngineError> {
        op.validate()?;
        let _lock = self.lock(&op.id)?;
        let Some(mut journal) = self.load_for(op, &op.fingerprint())? else {
            return Ok(());
        };
        match journal.status {
            Status::RolledBack => return Ok(()),
            Status::Completed | Status::Finalizing => {
                return Err(EngineError::AlreadyCompleted(op.id.clone()))
            }
            _ if journal.fingerprint != op.fingerprint() => {
                return Err(EngineError::PlanChanged {
                    operation: op.id.clone(),
                })
            }
            _ => {}
        }
        let reason = journal
            .failure
            .get_or_insert_with(|| "aborted".to_owned())
            .clone();
        journal.status = Status::RollingBack;
        self.save(&mut journal)?;
        observer.event(&Event::RollbackStarted { reason: &reason });
        self.undo_all(op, &mut journal, context, observer)
            .await?
            .map_err(EngineError::RollbackFailed)
    }

    /// Deletes an operation's journal, whatever its state. Only for a completed
    /// operation, or after the instance was repaired by other means: an
    /// unfinished operation is no longer resumable or abortable afterwards.
    pub fn forget(&self, operation: &str) -> Result<(), EngineError> {
        let _lock = self.lock(operation)?;
        fsutil::remove_file(&self.journal_path(operation))
    }

    /// Undoes started steps in reverse order. The outer error is a journal
    /// failure; the inner one the first undo that failed (the rollback stops
    /// there: later undos may depend on it).
    async fn undo_all<C: Sync>(
        &self,
        op: &Operation<C>,
        journal: &mut Journal,
        context: &C,
        observer: &dyn Observer,
    ) -> Result<Result<(), RollbackFailure>, EngineError> {
        // A rollback is never cancelled: stopping halfway helps nobody.
        let never = CancelToken::new();
        let run = journal.run.clone();
        for (index, step) in op.steps.iter().enumerate().rev() {
            let state = journal.steps[index].state;
            if !state.needs_undo() {
                continue;
            }
            let cx = StepContext {
                context,
                operation: &op.id,
                step: step.id(),
                run: &run,
                resumed: state == StepState::Running,
                observer,
                cancel: &never,
            };
            if let Err(source) = step.undo(&cx).await {
                let original = journal.failure.take().unwrap_or_default();
                journal.failure = Some(format!(
                    "{original}; rollback failed at step '{}': {source}",
                    step.id()
                ));
                journal.status = Status::RollbackFailed;
                self.save(journal)?;
                return Ok(Err(RollbackFailure {
                    step: step.id().to_owned(),
                    source,
                }));
            }
            journal.steps[index].state = StepState::Undone;
            self.save(journal)?;
            observer.event(&Event::StepUndone { step: step.id() });
        }
        journal.status = Status::RolledBack;
        self.save(journal)?;
        Ok(Ok(()))
    }

    async fn finalize_all<C: Sync>(
        &self,
        op: &Operation<C>,
        journal: &mut Journal,
        context: &C,
        observer: &dyn Observer,
    ) -> Result<(), EngineError> {
        journal.status = Status::Finalizing;
        self.save(journal)?;
        let never = CancelToken::new();
        let run = journal.run.clone();
        for step in &op.steps {
            let cx = StepContext {
                context,
                operation: &op.id,
                step: step.id(),
                run: &run,
                resumed: false,
                observer,
                cancel: &never,
            };
            if let Err(error) = step.finalize(&cx).await {
                observer.event(&Event::FinalizeFailed {
                    step: step.id(),
                    error: error.as_ref(),
                });
            }
        }
        journal.status = Status::Completed;
        self.save(journal)
    }

    fn journal_path(&self, operation: &str) -> PathBuf {
        self.state_dir.join(format!("{operation}.journal.json"))
    }

    fn load(&self, operation: &str) -> Result<Option<Journal>, EngineError> {
        let path = self.journal_path(operation);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(EngineError::io(&path)(e)),
        };
        let corrupt = |message: String| EngineError::CorruptJournal {
            path: path.clone(),
            message,
        };
        let journal: Journal =
            serde_json::from_slice(&bytes).map_err(|e| corrupt(e.to_string()))?;
        if journal.format != FORMAT {
            return Err(corrupt(format!("unsupported format {}", journal.format)));
        }
        Ok(Some(journal))
    }

    /// Loads the journal of `op`; one claiming the same plan must have its steps.
    fn load_for<C>(
        &self,
        op: &Operation<C>,
        fingerprint: &str,
    ) -> Result<Option<Journal>, EngineError> {
        let journal = self.load(&op.id)?;
        if let Some(j) = &journal {
            let same_steps = j.steps.len() == op.steps.len()
                && j.steps.iter().zip(&op.steps).all(|(r, s)| r.id == s.id());
            if j.fingerprint == fingerprint && !same_steps {
                return Err(EngineError::CorruptJournal {
                    path: self.journal_path(&op.id),
                    message: "steps do not match the plan fingerprint".to_owned(),
                });
            }
        }
        Ok(journal)
    }

    fn save(&self, journal: &mut Journal) -> Result<(), EngineError> {
        journal.updated = now();
        std::fs::create_dir_all(&self.state_dir).map_err(EngineError::io(&self.state_dir))?;
        let json = serde_json::to_vec_pretty(journal).expect("journal serializes");
        fsutil::write_atomic(&self.journal_path(&journal.operation), &json)
    }

    /// Exclusive OS lock, released when the file is dropped (also on crash).
    fn lock(&self, operation: &str) -> Result<std::fs::File, EngineError> {
        std::fs::create_dir_all(&self.state_dir).map_err(EngineError::io(&self.state_dir))?;
        let path = self.state_dir.join(format!("{operation}.lock"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(EngineError::io(&path))?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => Err(EngineError::Busy(operation.to_owned())),
            Err(std::fs::TryLockError::Error(e)) => Err(EngineError::io(&path)(e)),
        }
    }
}

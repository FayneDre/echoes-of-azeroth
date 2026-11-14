//! Engine behaviour: journal, resume, crash, rollback, repair, locking.

use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;
use std::sync::Mutex;

use eoa_engine::{
    CancelToken, Engine, EngineError, Event, Observer, Operation, Outcome, Status, Step,
    StepContext, StepFuture, StepState,
};
use futures_util::FutureExt;

/// Shared test context: what steps did, and how they should misbehave.
#[derive(Default)]
struct Ctx {
    log: Mutex<Vec<String>>,
    fail_run: Mutex<BTreeSet<String>>,
    fail_undo: Mutex<BTreeSet<String>>,
    panic_in: Mutex<BTreeSet<String>>,
    cancel_in: Mutex<Option<(String, CancelToken)>>,
}

impl Ctx {
    fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut self.log.lock().unwrap())
    }
    fn set(set: &Mutex<BTreeSet<String>>, id: &str, on: bool) {
        let mut set = set.lock().unwrap();
        if on {
            set.insert(id.to_owned());
        } else {
            set.remove(id);
        }
    }
}

struct Probe {
    id: String,
    param: String,
}

fn probe(id: &str) -> Probe {
    Probe {
        id: id.to_owned(),
        param: "v1".to_owned(),
    }
}

impl Step<Ctx> for Probe {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        "probe"
    }
    fn fingerprint(&self) -> String {
        self.param.clone()
    }
    fn run<'a>(&'a self, cx: &'a StepContext<'a, Ctx>) -> StepFuture<'a> {
        Box::pin(async move {
            let ctx = cx.context();
            let resumed = if cx.is_resumed() { " (resumed)" } else { "" };
            ctx.log
                .lock()
                .unwrap()
                .push(format!("run {}{resumed}", self.id));
            if ctx.panic_in.lock().unwrap().contains(&self.id) {
                panic!("simulated crash in {}", self.id);
            }
            if let Some((id, token)) = &*ctx.cancel_in.lock().unwrap() {
                if *id == self.id {
                    token.cancel();
                }
            }
            cx.check_cancelled()?;
            if ctx.fail_run.lock().unwrap().contains(&self.id) {
                return Err(format!("{} broke", self.id).into());
            }
            Ok(())
        })
    }
    fn undo<'a>(&'a self, cx: &'a StepContext<'a, Ctx>) -> StepFuture<'a> {
        Box::pin(async move {
            let ctx = cx.context();
            ctx.log.lock().unwrap().push(format!("undo {}", self.id));
            if ctx.fail_undo.lock().unwrap().contains(&self.id) {
                return Err(format!("cannot undo {}", self.id).into());
            }
            Ok(())
        })
    }
    fn finalize<'a>(&'a self, cx: &'a StepContext<'a, Ctx>) -> StepFuture<'a> {
        Box::pin(async move {
            cx.context()
                .log
                .lock()
                .unwrap()
                .push(format!("finalize {}", self.id));
            Ok(())
        })
    }
}

fn plan() -> Operation<Ctx> {
    Operation::new("instance-test")
        .step(probe("a"))
        .step(probe("b"))
        .step(probe("c"))
}

fn setup() -> (tempfile::TempDir, Engine, Ctx) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(dir.path().join("state"));
    (dir, engine, Ctx::default())
}

fn states(engine: &Engine) -> (Status, Vec<StepState>) {
    let journal = engine.status("instance-test").unwrap().unwrap();
    (
        journal.status,
        journal.steps.iter().map(|s| s.state).collect(),
    )
}

async fn run(engine: &Engine, op: &Operation<Ctx>, ctx: &Ctx) -> Result<Outcome, EngineError> {
    engine.run(op, ctx, &(), &CancelToken::new()).await
}

#[tokio::test]
async fn runs_then_is_idempotent() {
    let (_dir, engine, ctx) = setup();
    let op = plan();
    assert_eq!(run(&engine, &op, &ctx).await.unwrap(), Outcome::Completed);
    assert_eq!(
        ctx.take_log(),
        [
            "run a",
            "run b",
            "run c",
            "finalize a",
            "finalize b",
            "finalize c"
        ]
    );
    assert_eq!(states(&engine).0, Status::Completed);

    assert_eq!(
        run(&engine, &op, &ctx).await.unwrap(),
        Outcome::AlreadyCompleted
    );
    assert!(ctx.take_log().is_empty());
}

#[tokio::test]
async fn failure_rolls_back_in_reverse_order() {
    let (_dir, engine, ctx) = setup();
    Ctx::set(&ctx.fail_run, "c", true);
    let err = run(&engine, &plan(), &ctx).await.unwrap_err();
    match &err {
        EngineError::StepFailed { step, rollback, .. } => {
            assert_eq!(step, "c");
            assert!(rollback.is_none());
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(err.to_string().contains("c broke"), "{err}");
    // The failed step is undone too: it may have half-done its work.
    assert_eq!(
        ctx.take_log(),
        ["run a", "run b", "run c", "undo c", "undo b", "undo a"]
    );
    assert_eq!(
        states(&engine),
        (Status::RolledBack, vec![StepState::Undone; 3])
    );

    // Once the cause is fixed, the same plan runs again from scratch.
    Ctx::set(&ctx.fail_run, "c", false);
    assert_eq!(
        run(&engine, &plan(), &ctx).await.unwrap(),
        Outcome::Completed
    );
    assert_eq!(&ctx.take_log()[..3], ["run a", "run b", "run c"]);
}

#[tokio::test]
async fn cancel_then_resume() {
    let (_dir, engine, ctx) = setup();
    let token = CancelToken::new();
    *ctx.cancel_in.lock().unwrap() = Some(("b".to_owned(), token.clone()));
    let outcome = engine.run(&plan(), &ctx, &(), &token).await.unwrap();
    assert_eq!(outcome, Outcome::Cancelled);
    assert_eq!(ctx.take_log(), ["run a", "run b"]);
    assert_eq!(
        states(&engine),
        (
            Status::Running,
            vec![StepState::Done, StepState::Running, StepState::Pending]
        )
    );

    *ctx.cancel_in.lock().unwrap() = None;
    assert_eq!(
        run(&engine, &plan(), &ctx).await.unwrap(),
        Outcome::Completed
    );
    assert_eq!(
        &ctx.take_log()[..2],
        ["run b (resumed)", "run c"],
        "a is skipped, b is told it resumes"
    );
}

#[tokio::test]
async fn crash_mid_step_resumes() {
    let (_dir, engine, ctx) = setup();
    Ctx::set(&ctx.panic_in, "b", true);
    let crashed = AssertUnwindSafe(run(&engine, &plan(), &ctx))
        .catch_unwind()
        .await;
    assert!(crashed.is_err(), "the step panicked");
    // The lock died with the run; the journal shows where it stopped.
    assert_eq!(
        states(&engine),
        (
            Status::Running,
            vec![StepState::Done, StepState::Running, StepState::Pending]
        )
    );
    ctx.take_log();

    Ctx::set(&ctx.panic_in, "b", false);
    assert_eq!(
        run(&engine, &plan(), &ctx).await.unwrap(),
        Outcome::Completed
    );
    assert_eq!(&ctx.take_log()[..2], ["run b (resumed)", "run c"]);
}

#[tokio::test]
async fn crash_during_rollback_resumes_the_rollback() {
    let (_dir, engine, ctx) = setup();
    Ctx::set(&ctx.fail_run, "c", true);
    Ctx::set(&ctx.fail_undo, "b", true);
    let err = run(&engine, &plan(), &ctx).await.unwrap_err();
    assert!(
        matches!(&err, EngineError::StepFailed { rollback: Some(f), .. } if f.step == "b"),
        "{err:?}"
    );
    assert_eq!(states(&engine).0, Status::RollbackFailed);
    ctx.take_log();

    // Blocked until repaired: no new run on top of an unknown state.
    let blocked = run(&engine, &plan(), &ctx).await.unwrap_err();
    assert!(
        matches!(&blocked, EngineError::NeedsRepair { failure, .. } if failure.contains("cannot undo b")),
        "{blocked:?}"
    );

    // Retrying the rollback after the cause is fixed finishes it; c stays undone.
    Ctx::set(&ctx.fail_undo, "b", false);
    engine.abort(&plan(), &ctx, &()).await.unwrap();
    assert_eq!(ctx.take_log(), ["undo b", "undo a"]);
    assert_eq!(
        states(&engine),
        (Status::RolledBack, vec![StepState::Undone; 3])
    );
}

#[tokio::test]
async fn interrupted_rollback_finishes_on_next_run() {
    let (_dir, engine, ctx) = setup();
    Ctx::set(&ctx.fail_run, "b", true);
    // Crash in the middle of the rollback: undo of `a` panics.
    struct PanicUndo;
    impl Step<Ctx> for PanicUndo {
        fn id(&self) -> &str {
            "a"
        }
        fn kind(&self) -> &str {
            "probe"
        }
        fn fingerprint(&self) -> String {
            "v1".to_owned()
        }
        fn run<'a>(&'a self, _cx: &'a StepContext<'a, Ctx>) -> StepFuture<'a> {
            Box::pin(async { Ok(()) })
        }
        fn undo<'a>(&'a self, _cx: &'a StepContext<'a, Ctx>) -> StepFuture<'a> {
            Box::pin(async { panic!("crash during rollback") })
        }
    }
    let crashing = Operation::new("instance-test")
        .step(PanicUndo)
        .step(probe("b"))
        .step(probe("c"));
    let crashed = AssertUnwindSafe(run(&engine, &crashing, &ctx))
        .catch_unwind()
        .await;
    assert!(crashed.is_err());
    assert_eq!(states(&engine).0, Status::RollingBack);
    ctx.take_log();

    // Same plan (same fingerprint), healthy undo: the rollback completes.
    let outcome = run(&engine, &plan(), &ctx).await.unwrap();
    assert!(
        matches!(&outcome, Outcome::RolledBack { failure } if failure.contains("b broke")),
        "{outcome:?}"
    );
    assert_eq!(ctx.take_log(), ["undo a"]);
}

#[tokio::test]
async fn changed_plan_is_refused_while_unfinished() {
    let (_dir, engine, ctx) = setup();
    let token = CancelToken::new();
    *ctx.cancel_in.lock().unwrap() = Some(("b".to_owned(), token.clone()));
    engine.run(&plan(), &ctx, &(), &token).await.unwrap();
    *ctx.cancel_in.lock().unwrap() = None;

    let mut changed = Operation::new("instance-test").step(probe("a"));
    changed.push(Box::new(Probe {
        id: "b".to_owned(),
        param: "v2".to_owned(),
    }));
    let err = run(&engine, &changed, &ctx).await.unwrap_err();
    assert!(matches!(err, EngineError::PlanChanged { .. }), "{err:?}");
    let err = engine.abort(&changed, &ctx, &()).await.unwrap_err();
    assert!(matches!(err, EngineError::PlanChanged { .. }), "{err:?}");

    // Aborting with the original plan undoes the started steps (b was interrupted).
    ctx.take_log();
    engine.abort(&plan(), &ctx, &()).await.unwrap();
    assert_eq!(ctx.take_log(), ["undo b", "undo a"]);

    // A new plan is accepted once the old operation is over.
    assert_eq!(
        run(&engine, &changed, &ctx).await.unwrap(),
        Outcome::Completed
    );
    let err = engine.abort(&changed, &ctx, &()).await.unwrap_err();
    assert!(matches!(err, EngineError::AlreadyCompleted(_)), "{err:?}");
}

#[tokio::test]
async fn concurrent_run_is_busy() {
    let (_dir, engine, ctx) = setup();
    struct Nested<'e>(&'e Engine);
    impl Step<Ctx> for Nested<'static> {
        fn id(&self) -> &str {
            "nested"
        }
        fn kind(&self) -> &str {
            "probe"
        }
        fn run<'a>(&'a self, cx: &'a StepContext<'a, Ctx>) -> StepFuture<'a> {
            Box::pin(async move {
                let err = self
                    .0
                    .run(&plan(), cx.context(), &(), &CancelToken::new())
                    .await;
                match err {
                    Err(EngineError::Busy(id)) => {
                        cx.context().log.lock().unwrap().push(format!("busy {id}"));
                        Ok(())
                    }
                    other => Err(format!("expected Busy, got {other:?}").into()),
                }
            })
        }
    }
    let engine: &'static Engine = Box::leak(Box::new(engine));
    let op = Operation::new("instance-test").step(Nested(engine));
    assert_eq!(run(engine, &op, &ctx).await.unwrap(), Outcome::Completed);
    assert_eq!(ctx.take_log(), ["busy instance-test"]);
}

#[tokio::test]
async fn invalid_operations_are_rejected() {
    let (_dir, engine, ctx) = setup();
    for op in [
        Operation::new("../escape").step(probe("a")),
        Operation::new("").step(probe("a")),
        Operation::new("ok").step(probe("a")).step(probe("a")),
    ] {
        let err = run(&engine, &op, &ctx).await.unwrap_err();
        assert!(matches!(err, EngineError::InvalidOperation(_)), "{err:?}");
    }
}

#[tokio::test]
async fn corrupt_journal_is_reported() {
    let (_dir, engine, ctx) = setup();
    std::fs::create_dir_all(engine.state_dir()).unwrap();
    std::fs::write(
        engine.state_dir().join("instance-test.journal.json"),
        b"{ not json",
    )
    .unwrap();
    let err = run(&engine, &plan(), &ctx).await.unwrap_err();
    assert!(matches!(err, EngineError::CorruptJournal { .. }), "{err:?}");
    engine.forget("instance-test").unwrap();
    assert_eq!(
        run(&engine, &plan(), &ctx).await.unwrap(),
        Outcome::Completed
    );
}

#[tokio::test]
async fn events_describe_progress() {
    #[derive(Default)]
    struct Events(Mutex<Vec<String>>);
    impl Observer for Events {
        fn event(&self, event: &Event<'_>) {
            let line = match event {
                Event::StepStarted {
                    index,
                    total,
                    step,
                    resumed,
                    ..
                } => format!("start {step} {}/{total} {resumed}", index + 1),
                Event::StepSkipped { step, .. } => format!("skip {step}"),
                Event::StepFinished { step, .. } => format!("done {step}"),
                Event::StepProgress { .. } => "progress".to_owned(),
                Event::RollbackStarted { reason } => format!("rollback: {reason}"),
                Event::StepUndone { step } => format!("undone {step}"),
                Event::FinalizeFailed { step, .. } => format!("finalize failed {step}"),
            };
            self.0.lock().unwrap().push(line);
        }
    }
    let (_dir, engine, ctx) = setup();
    Ctx::set(&ctx.fail_run, "b", true);
    let events = Events::default();
    let op = Operation::new("instance-test")
        .step(probe("a"))
        .step(probe("b"));
    engine
        .run(&op, &ctx, &events, &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        *events.0.lock().unwrap(),
        [
            "start a 1/2 false",
            "done a",
            "start b 2/2 false",
            "rollback: step 'b' failed: b broke",
            "undone b",
            "undone a",
        ]
    );
}

#[tokio::test]
async fn run_id_is_stable_across_resume_and_new_per_execution() {
    let (_dir, engine, ctx) = setup();
    let token = CancelToken::new();
    *ctx.cancel_in.lock().unwrap() = Some(("b".to_owned(), token.clone()));
    engine.run(&plan(), &ctx, &(), &token).await.unwrap();
    let first = engine.status("instance-test").unwrap().unwrap().run;
    *ctx.cancel_in.lock().unwrap() = None;
    run(&engine, &plan(), &ctx).await.unwrap();
    assert_eq!(engine.status("instance-test").unwrap().unwrap().run, first);

    // A new execution of the same id (here: after a failure) gets a new run id.
    Ctx::set(&ctx.fail_run, "a", true);
    let changed = Operation::new("instance-test").step(probe("a"));
    run(&engine, &changed, &ctx).await.unwrap_err();
    assert_ne!(engine.status("instance-test").unwrap().unwrap().run, first);
}

#[tokio::test]
async fn journal_not_matching_its_fingerprint_is_corrupt() {
    let (_dir, engine, ctx) = setup();
    let token = CancelToken::new();
    *ctx.cancel_in.lock().unwrap() = Some(("b".to_owned(), token.clone()));
    engine.run(&plan(), &ctx, &(), &token).await.unwrap();
    *ctx.cancel_in.lock().unwrap() = None;

    let path = engine.state_dir().join("instance-test.journal.json");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    journal["steps"].as_array_mut().unwrap().pop();
    std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();

    let err = run(&engine, &plan(), &ctx).await.unwrap_err();
    assert!(matches!(err, EngineError::CorruptJournal { .. }), "{err:?}");
}

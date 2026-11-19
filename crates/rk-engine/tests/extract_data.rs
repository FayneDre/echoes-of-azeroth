//! `extract-data` with stand-in tools: copies of the system shell running
//! a one-line script, so the tests need no game client and no server build.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rk_core::manifest::{DataExtraction, ExtractionStep};
use rk_core::LocalizedText;
use rk_engine::steps::{ExtractData, ExtractDataError};
use rk_engine::{CancelToken, Engine, EngineError, Operation, Outcome};

#[cfg(windows)]
const SHELL: (&str, &str) = ("cmd.exe", "/C");
#[cfg(not(windows))]
const SHELL: (&str, &str) = ("sh", "-c");

#[cfg(windows)]
fn sleep_script() -> &'static str {
    // A wait inside cmd itself: a child process (ping, timeout) would
    // outlive the killed shell and keep the work directory open.
    "for /L %i in (1,1,2000000000) do @cd ."
}
#[cfg(not(windows))]
fn sleep_script() -> &'static str {
    "sleep 60"
}

/// Copies the system shell to `<tools>/bin/<name>` (a tool of the "server build").
fn install_shell(tools: &Path, name: &str) -> String {
    #[cfg(windows)]
    let source = PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32/cmd.exe");
    #[cfg(not(windows))]
    let source = PathBuf::from("/bin/sh");
    let bin = tools.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    fs::copy(source, bin.join(&file)).unwrap();
    format!("bin/{file}")
}

fn step(id: &str, executable: &str, script: &str, create: &[&str]) -> ExtractionStep {
    ExtractionStep {
        id: id.into(),
        executable: executable.into(),
        args: vec![SHELL.1.into(), script.into()],
        create: create.iter().map(|c| (*c).into()).collect(),
        weight: 1.0,
        label: LocalizedText {
            en: id.into(),
            fr: None,
        },
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    tools: PathBuf,
    client: PathBuf,
    target: PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let tools = root.join("instance");
    let client = root.join("game");
    fs::create_dir_all(client.join("Data/enUS")).unwrap();
    let target = root.join("store/server-data/extract-1");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    Setup {
        _dir: dir,
        root,
        tools,
        client,
        target,
    }
}

fn extraction(steps: Vec<ExtractionStep>) -> DataExtraction {
    DataExtraction {
        version: "1".into(),
        locale: "enUS".into(),
        max_client_path: None,
        unpacked_size: None,
        steps,
        remove: vec!["Buildings".into()],
        contents: vec!["maps".into(), "vmaps".into()],
    }
}

fn data_step(s: &Setup, extraction: DataExtraction) -> ExtractData {
    ExtractData {
        id: "extract-data".into(),
        extraction,
        tools_root: s.tools.clone(),
        client_root: s.client.clone(),
        target: s.target.clone(),
        logs: s.root.join("logs"),
        cpus: 4,
    }
}

async fn run(s: &Setup, step: ExtractData, cancel: &CancelToken) -> Result<Outcome, EngineError> {
    let op = Operation::new("serverdata-test").step(step);
    Engine::new(s.root.join("state"))
        .run(&op, &(), &(), cancel)
        .await
}

fn step_error(e: &EngineError) -> &ExtractDataError {
    match e {
        EngineError::StepFailed { source, .. } => {
            source.downcast_ref().expect("extract-data error")
        }
        other => panic!("not a step failure: {other}"),
    }
}

#[tokio::test]
async fn tools_run_in_order_in_a_partial_dir_then_it_takes_its_place() {
    let s = setup();
    let tool = install_shell(&s.tools, "tool");
    let ex = extraction(vec![
        step("maps", &tool, "mkdir maps && mkdir Buildings", &[]),
        // `create` made vmaps before the tool ran; it writes into it.
        step("vmaps", &tool, "echo done>vmaps/ok.txt", &["vmaps"]),
    ]);
    let outcome = run(&s, data_step(&s, ex), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(outcome, Outcome::Completed);
    assert!(s.target.join("maps").is_dir());
    assert!(s.target.join("vmaps/ok.txt").is_file());
    // Intermediate and bookkeeping directories are gone; nothing is left aside.
    assert!(!s.target.join("Buildings").exists());
    assert!(!s.target.join(".rk-steps").exists());
    assert!(!s.target.with_file_name("extract-1.partial").exists());
    assert!(s.root.join("logs/maps.log").is_file());
}

#[tokio::test]
async fn a_cancelled_extraction_resumes_after_its_finished_tools() {
    let s = setup();
    let tool = install_shell(&s.tools, "tool");
    let go = s.root.join("go");
    #[cfg(windows)]
    let second = format!(
        // Relative and unquoted: Rust escapes quotes as \", which cmd.exe
        // does not understand. The work dir is store/server-data/<x>.partial.
        "if exist ..\\..\\..\\go (mkdir vmaps) else ({})",
        sleep_script()
    );
    #[cfg(not(windows))]
    let second = format!(
        "if [ -e ../../../go ]; then mkdir vmaps; else {}; fi",
        sleep_script()
    );
    let ex = extraction(vec![
        step("maps", &tool, "mkdir maps && echo x>>../../runs.txt", &[]),
        step("vmaps", &tool, &second, &[]),
    ]);

    let cancel = CancelToken::new();
    let (outcome, ()) = tokio::join!(run(&s, data_step(&s, ex.clone()), &cancel), async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        cancel.cancel();
    });
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert!(!s.target.exists());
    assert!(s.target.with_file_name("extract-1.partial/maps").is_dir());

    fs::write(&go, b"").unwrap();
    let outcome = run(&s, data_step(&s, ex), &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(outcome, Outcome::Completed);
    assert!(s.target.join("vmaps").is_dir());
    // The first tool ran once: its result was kept.
    let runs = fs::read_to_string(s.root.join("store/runs.txt")).unwrap();
    assert_eq!(runs.lines().count(), 1, "{runs:?}");
}

#[tokio::test]
async fn a_failing_tool_rolls_back_with_the_end_of_its_log() {
    let s = setup();
    let tool = install_shell(&s.tools, "tool");
    let ex = extraction(vec![
        step("maps", &tool, "mkdir maps", &[]),
        step("vmaps", &tool, "echo cannot open Data && exit 3", &[]),
    ]);
    let err = run(&s, data_step(&s, ex), &CancelToken::new())
        .await
        .unwrap_err();
    match step_error(&err) {
        ExtractDataError::ToolFailed {
            step, code, tail, ..
        } => {
            assert_eq!(step, "vmaps");
            assert_eq!(code, "3");
            assert!(tail.contains("cannot open Data"), "{tail}");
        }
        other => panic!("{other}"),
    }
    assert!(!s.target.exists());
    assert!(!s.target.with_file_name("extract-1.partial").exists());
}

#[tokio::test]
async fn a_result_without_its_contents_is_refused() {
    let s = setup();
    let tool = install_shell(&s.tools, "tool");
    let ex = extraction(vec![step("maps", &tool, "mkdir maps", &[])]);
    let err = run(&s, data_step(&s, ex), &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        step_error(&err),
        ExtractDataError::Incomplete { entry, .. } if entry == "vmaps"
    ));
    assert!(!s.target.exists());
}

#[tokio::test]
async fn the_client_is_checked_before_any_tool_runs() {
    let s = setup();
    let tool = install_shell(&s.tools, "tool");
    let mut ex = extraction(vec![step("maps", &tool, "mkdir maps", &[])]);
    ex.locale = "frFR".into();
    let err = run(&s, data_step(&s, ex.clone()), &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        step_error(&err),
        ExtractDataError::MissingLocale { locale, .. } if locale == "frFR"
    ));

    ex.locale = "enUS".into();
    ex.max_client_path = Some(8);
    let err = run(&s, data_step(&s, ex), &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        step_error(&err),
        ExtractDataError::ClientPathTooLong { max: 8, .. }
    ));
    assert!(!s.root.join("logs").exists(), "no tool ran");
}

#[tokio::test]
async fn a_missing_tool_is_named() {
    let s = setup();
    let ex = extraction(vec![step("maps", "bin/mapextractor.exe", "", &[])]);
    let err = run(&s, data_step(&s, ex), &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        step_error(&err),
        ExtractDataError::MissingTool { tool } if tool.ends_with("bin/mapextractor.exe")
    ));
}

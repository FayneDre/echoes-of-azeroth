//! DirSwap: install, upgrade, undo, finalize and every crash point in between.

use std::fs;
use std::path::Path;

use eoa_engine::DirSwap;

fn write(dir: &Path, content: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("version.txt"), content).unwrap();
}

fn version(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join("version.txt")).ok()
}

/// Names in the parent directory, sorted: shows every leftover.
fn listing(parent: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn setup() -> (tempfile::TempDir, DirSwap) {
    let dir = tempfile::tempdir().unwrap();
    let swap = DirSwap::new(dir.path().join("bin"), "run-1").unwrap();
    (dir, swap)
}

fn stage(swap: &DirSwap, content: &str) {
    let staging = swap.prepare().unwrap();
    write(&staging, content);
}

/// Marks rollback data as belonging to `run`, as a commit of that run would.
fn own(dir: &Path, run: &str) {
    fs::write(dir.join("bin.eoa-owner"), run).unwrap();
}

#[test]
fn fresh_install_commit_undo_finalize() {
    let (dir, swap) = setup();
    stage(&swap, "v1");
    swap.commit().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"));

    swap.undo().unwrap();
    assert_eq!(listing(dir.path()), Vec::<String>::new(), "nothing left");

    stage(&swap, "v1");
    swap.commit().unwrap();
    swap.finalize().unwrap();
    assert_eq!(listing(dir.path()), ["bin"]);
    // Undo after finalize keeps the new version: it is permanent.
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"));
}

#[test]
fn upgrade_undo_restores_previous() {
    let (dir, swap) = setup();
    write(swap.target(), "v1");
    stage(&swap, "v2");
    swap.commit().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v2"));
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"));
    assert_eq!(listing(dir.path()), ["bin"]);
}

#[test]
fn resumed_commit_keeps_the_original_version() {
    let (dir, swap) = setup();
    write(swap.target(), "v1");
    stage(&swap, "v2");
    swap.commit().unwrap();
    // Crash before the step was journaled as done: the step runs again.
    stage(&swap, "v2");
    swap.commit().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v2"));
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"), "v1, not v2");
    assert_eq!(listing(dir.path()), ["bin"]);
}

#[test]
fn resumed_fresh_install_undo_removes_everything() {
    let (dir, swap) = setup();
    stage(&swap, "v1");
    swap.commit().unwrap();
    stage(&swap, "v1");
    swap.commit().unwrap();
    swap.undo().unwrap();
    assert_eq!(listing(dir.path()), Vec::<String>::new());
}

#[test]
fn crash_after_seal_is_finished_forward() {
    let (dir, swap) = setup();
    write(swap.target(), "v1");
    own(dir.path(), "run-1");
    // Sealed but not swapped yet.
    write(&dir.path().join("bin.eoa-ready"), "v2");
    swap.recover().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v2"));
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"));
    assert_eq!(listing(dir.path()), ["bin"]);
}

#[test]
fn crash_between_the_two_renames_is_finished_forward() {
    let (dir, swap) = setup();
    own(dir.path(), "run-1");
    // Target moved aside, sealed copy not moved in yet: no `bin` at all.
    write(&dir.path().join("bin.eoa-previous"), "v1");
    write(&dir.path().join("bin.eoa-ready"), "v2");
    swap.recover().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v2"));
    swap.finalize().unwrap();
    assert_eq!(listing(dir.path()), ["bin"]);
}

#[test]
fn unsealed_staging_is_never_installed() {
    let (dir, swap) = setup();
    write(swap.target(), "v1");
    write(&swap.staging(), "half-extracted");
    swap.recover().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"));
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"));
    assert_eq!(listing(dir.path()), ["bin"]);
}

#[test]
fn crash_during_undo_never_loses_both_versions() {
    let (dir, swap) = setup();
    own(dir.path(), "run-1");
    // Undo moved v2 to the trash, crashed before restoring v1.
    write(&dir.path().join("bin.eoa-previous"), "v1");
    write(&dir.path().join("bin.eoa-trash"), "v2");
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v1"));
    assert_eq!(listing(dir.path()), ["bin"]);

    // Same state found by another execution (journal forgotten): restored too.
    own(dir.path(), "run-1");
    write(&dir.path().join("bin.eoa-previous"), "v0");
    fs::remove_dir_all(swap.target()).unwrap();
    let other = DirSwap::new(swap.target(), "run-2").unwrap();
    stage(&other, "v3");
    assert_eq!(version(other.target()).as_deref(), Some("v0"));
}

#[test]
fn leftovers_of_another_execution_are_never_restored() {
    let (dir, swap) = setup();
    write(swap.target(), "v2");
    // An earlier, completed execution failed to clean up.
    write(&dir.path().join("bin.eoa-previous"), "v1");
    own(dir.path(), "run-0");

    // Failing before anything was staged: the undo must keep v2.
    swap.undo().unwrap();
    assert_eq!(
        version(swap.target()).as_deref(),
        Some("v2"),
        "no downgrade"
    );
    assert_eq!(listing(dir.path()), ["bin"]);

    // Same with a commit in between: the undo brings v2 back, not v1.
    write(&dir.path().join("bin.eoa-previous"), "v1");
    own(dir.path(), "run-0");
    stage(&swap, "v3");
    swap.commit().unwrap();
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v2"), "v2, not v1");

    // Leftovers without any owner marker are stale too.
    write(&dir.path().join("bin.eoa-previous"), "v1");
    swap.undo().unwrap();
    assert_eq!(version(swap.target()).as_deref(), Some("v2"));
    assert_eq!(listing(dir.path()), ["bin"]);
}

#[test]
fn invalid_arguments_fail() {
    let (dir, swap) = setup();
    assert!(swap.commit().is_err(), "nothing staged");
    assert!(DirSwap::new("", "run-1").is_err());
    assert!(DirSwap::new(dir.path().join("bin"), "").is_err());
}

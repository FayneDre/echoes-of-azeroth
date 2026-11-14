//! Built-in steps: download (from a pre-filled store) and extract.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eoa_engine::steps::{Extract, ExtractError, Fetch, Strip};
use eoa_engine::{
    CancelToken, Engine, EngineError, Operation, Outcome, Step, StepContext, StepFuture,
};
use eoa_store::{FetchRequest, Store};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

/// Builds a ZIP; names ending in `/` are directories.
fn make_zip(path: &Path, entries: &[(&str, &[u8])], method: zip::CompressionMethod) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options = SimpleFileOptions::default().compression_method(method);
    for (name, data) in entries {
        if name.ends_with('/') {
            zip.add_directory(*name, options).unwrap();
        } else {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
    }
    zip.finish().unwrap();
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Runs a one-step operation.
async fn extract(step: Extract, state: &Path) -> Result<Outcome, EngineError> {
    let op = Operation::new("extract-test").step(step);
    Engine::new(state)
        .run(&op, &(), &(), &CancelToken::new())
        .await
}

#[tokio::test]
async fn extracts_in_parallel_and_strips_the_root_dir() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("mysql.zip");
    let big = vec![7u8; 3 * 1024 * 1024];
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("mysql-8.4.13-winx64/".into(), vec![]),
        ("mysql-8.4.13-winx64/bin/mysqld.exe".into(), big.clone()),
        ("mysql-8.4.13-winx64/share/empty/".into(), vec![]),
    ];
    for i in 0..60 {
        entries.push((
            format!("mysql-8.4.13-winx64/share/f{i}.txt"),
            format!("file {i}").into_bytes(),
        ));
    }
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    make_zip(&archive, &borrowed, zip::CompressionMethod::Deflated);

    let root = dir.path().join("components");
    let target = root.join("mysql-8-4");
    let step = Extract::new("extract", &archive, &target)
        .strip(Strip::SingleRootDir)
        .threads(4);
    let outcome = extract(step, &dir.path().join("state")).await.unwrap();
    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(fs::read(target.join("bin/mysqld.exe")).unwrap(), big);
    assert_eq!(
        fs::read_to_string(target.join("share/f42.txt")).unwrap(),
        "file 42"
    );
    assert!(target.join("share/empty").is_dir());
    assert_eq!(listing(&root), ["mysql-8-4"], "no staging leftovers");
}

#[tokio::test]
async fn unsafe_entries_are_rejected_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "../evil.txt",
        "a/../../evil.txt",
        "/abs/evil.txt",
        "a\\..\\..\\evil.txt",
        "C:/evil.txt",
        "\\\\server\\share\\evil.txt",
    ] {
        let archive = dir.path().join("evil.zip");
        make_zip(
            &archive,
            &[("ok.txt", b"ok"), (name, b"evil")],
            zip::CompressionMethod::Stored,
        );
        let target = dir.path().join("out").join("target");
        let err = extract(
            Extract::new("extract", &archive, &target),
            &dir.path().join("state"),
        )
        .await
        .map(|o| {
            let names = listing(&target);
            panic!("{name}: accepted ({o:?}), extracted {names:?}")
        })
        .unwrap_err();
        let EngineError::StepFailed { source, .. } = &err else {
            panic!("{err:?}")
        };
        assert!(
            matches!(
                source.downcast_ref::<ExtractError>(),
                Some(ExtractError::UnsafeEntry(_))
            ),
            "{name}: {err}"
        );
        assert!(!dir.path().join("evil.txt").exists());
        assert_eq!(listing(&dir.path().join("out")), Vec::<String>::new());
    }
}

#[tokio::test]
async fn corrupted_entry_fails_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("data.zip");
    let marker = b"MARKER-0123456789-MARKER";
    make_zip(
        &archive,
        &[("dbc/Spell.dbc", marker)],
        zip::CompressionMethod::Stored,
    );
    let mut bytes = fs::read(&archive).unwrap();
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .unwrap();
    bytes[at + 3] ^= 0xFF;
    fs::write(&archive, bytes).unwrap();

    let out = dir.path().join("out");
    let err = extract(
        Extract::new("extract", &archive, out.join("data")),
        &dir.path().join("state"),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, EngineError::StepFailed { .. }), "{err:?}");
    assert_eq!(listing(&out), Vec::<String>::new());
}

#[tokio::test]
async fn single_root_dir_is_required_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("data.zip");
    make_zip(
        &archive,
        &[("dbc/a", b"1"), ("maps/b", b"2")],
        zip::CompressionMethod::Stored,
    );
    let err = extract(
        Extract::new("extract", &archive, dir.path().join("data")).strip(Strip::SingleRootDir),
        &dir.path().join("state"),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("single root"), "{err}");
}

/// A step that always fails, to trigger a rollback after the extraction.
struct Broken;
impl Step<()> for Broken {
    fn id(&self) -> &str {
        "broken"
    }
    fn kind(&self) -> &str {
        "run-tool"
    }
    fn run<'a>(&'a self, _cx: &'a StepContext<'a, ()>) -> StepFuture<'a> {
        Box::pin(async { Err("tool failed".into()) })
    }
}

/// Puts `bytes` in the store as if downloaded, returning the request and blob path.
fn preload(store: &Store, bytes: &[u8]) -> (FetchRequest, PathBuf) {
    let sha256 = hex::encode(Sha256::digest(bytes));
    let blob = store.blob_path(&sha256);
    fs::create_dir_all(blob.parent().unwrap()).unwrap();
    fs::write(&blob, bytes).unwrap();
    let request = FetchRequest {
        urls: vec!["http://127.0.0.1:9/unused".into()],
        size: bytes.len() as u64,
        sha256,
    };
    (request, blob)
}

#[tokio::test]
async fn failed_upgrade_restores_the_installed_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("store")).unwrap());
    let engine = Engine::new(dir.path().join("state"));
    let target = dir.path().join("components").join("tool");

    let zip_bytes = |version: &str| {
        let path = dir.path().join(format!("{version}.zip"));
        make_zip(
            &path,
            &[("tool/version.txt", version.as_bytes())],
            zip::CompressionMethod::Deflated,
        );
        fs::read(path).unwrap()
    };
    let install = |version: &str, broken: bool| {
        let (request, blob) = preload(&store, &zip_bytes(version));
        let mut op = Operation::new("component-tool")
            .step(Fetch::new("download", store.clone(), request))
            .step(Extract::new("extract", blob, &target).strip(Strip::SingleRootDir));
        if broken {
            op.push(Box::new(Broken));
        }
        op
    };

    let v1 = install("v1", false);
    let outcome = engine.run(&v1, &(), &(), &CancelToken::new()).await;
    assert_eq!(outcome.unwrap(), Outcome::Completed);
    assert_eq!(
        fs::read_to_string(target.join("version.txt")).unwrap(),
        "v1"
    );

    let v2 = install("v2", true);
    let err = engine
        .run(&v2, &(), &(), &CancelToken::new())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, EngineError::StepFailed { step, rollback: None, .. } if step == "broken"),
        "{err:?}"
    );
    assert_eq!(
        fs::read_to_string(target.join("version.txt")).unwrap(),
        "v1",
        "rolled back to the installed version"
    );
    assert_eq!(listing(&dir.path().join("components")), ["tool"]);

    // The same upgrade without the broken step goes through.
    let v2 = install("v2", false);
    let outcome = engine.run(&v2, &(), &(), &CancelToken::new()).await;
    assert_eq!(outcome.unwrap(), Outcome::Completed);
    assert_eq!(
        fs::read_to_string(target.join("version.txt")).unwrap(),
        "v2"
    );
    assert_eq!(listing(&dir.path().join("components")), ["tool"]);
}

#[tokio::test]
async fn subdir_extracts_only_that_part_of_the_archive() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("server.zip");
    make_zip(
        &archive,
        &[
            ("build-info.json", b"{}"),
            ("dist/", b""),
            ("dist/worldserver.exe", b"exe"),
            ("dist/configs/worldserver.conf.dist", b"dist"),
            ("distractor/file", b"no"),
            ("sql/data/sql/base.sql", b"sql"),
        ],
        zip::CompressionMethod::Deflated,
    );
    let bin = dir.path().join("instance").join("bin");
    let step = Extract::new("extract-bin", &archive, &bin).subdir("dist");
    extract(step, &dir.path().join("state")).await.unwrap();
    assert_eq!(listing(&bin), ["configs", "worldserver.exe"]);
    assert_eq!(
        fs::read_to_string(bin.join("configs/worldserver.conf.dist")).unwrap(),
        "dist"
    );
}

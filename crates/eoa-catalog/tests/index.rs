//! Signed index (ADR 0010): a fixture signed by `tools/ci/index.mjs` with a
//! published *test* key, verified here, plus selection against the real catalog.

use std::path::PathBuf;

use eoa_catalog::index::{self, IndexError, TrustedKey};

/// Public half of the test key (seed: sha256 of "echoes-of-azeroth test index key").
const TEST_KEY: &str = "01344ac03f3521463b0c61714e875b0511834a8f5d147805ab207534a69b51b2";

fn test_key() -> TrustedKey {
    let bytes: [u8; 32] = hex::decode(TEST_KEY).unwrap().try_into().unwrap();
    TrustedKey::from_bytes(&bytes).unwrap()
}

fn fixture() -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/index-test.json"))
        .unwrap()
}

fn catalog() -> eoa_core::Catalog {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    eoa_catalog::load_dir(&root).expect("catalog loads")
}

/// The fixture with its payload edited (signature unchanged).
fn tampered(edit: impl Fn(&str) -> String) -> Vec<u8> {
    let mut envelope: serde_json::Value = serde_json::from_slice(&fixture()).unwrap();
    let payload = envelope["payload"].as_str().unwrap().to_owned();
    envelope["payload"] = serde_json::Value::String(edit(&payload));
    serde_json::to_vec(&envelope).unwrap()
}

#[test]
fn index_signed_by_the_ci_tool_verifies() {
    let index = index::open(&fixture(), &[test_key()]).unwrap();
    assert_eq!(index.serial, 7);
    assert_eq!(index.builds.len(), 3);
    assert_eq!(test_key().id, "5a23dd50dfb038ff");
}

#[test]
fn test_key_is_never_trusted_by_the_launcher() {
    assert!(index::trusted_keys().iter().all(|k| k.id != test_key().id));
    assert!(matches!(
        index::open(&fixture(), &index::trusted_keys()),
        Err(IndexError::UnknownKey(_))
    ));
}

#[test]
fn any_change_to_the_payload_breaks_the_signature() {
    for edit in [
        |p: &str| p.replace("\"serial\": 7", "\"serial\": 8"),
        |p: &str| p.replace("example.invalid", "evil.invalid"),
        |p: &str| format!("{p} "),
    ] {
        assert!(matches!(
            index::open(&tampered(edit), &[test_key()]),
            Err(IndexError::BadSignature)
        ));
    }
}

#[test]
fn zeroed_signature_is_refused() {
    // Well-formed (64 bytes) but not a signature of this payload.
    let mut envelope: serde_json::Value = serde_json::from_slice(&fixture()).unwrap();
    envelope["signature"] = serde_json::Value::String("00".repeat(64));
    let bytes = serde_json::to_vec(&envelope).unwrap();
    assert!(matches!(
        index::open(&bytes, &[test_key()]),
        Err(IndexError::BadSignature)
    ));
}

#[test]
fn older_or_rewritten_index_is_refused() {
    let current = index::open(&fixture(), &[test_key()]).unwrap();
    let mut older = current.clone();
    older.serial = 6;
    let mut rewritten = current.clone();
    rewritten.builds.pop();
    let mut newer = current.clone();
    newer.serial = 8;

    assert!(index::check_newer(&current, None).is_ok());
    assert!(index::check_newer(&current, Some(&current)).is_ok());
    assert!(index::check_newer(&newer, Some(&current)).is_ok());
    assert!(matches!(
        index::check_newer(&older, Some(&current)),
        Err(IndexError::Rollback {
            offered: 6,
            accepted: 7
        })
    ));
    assert!(matches!(
        index::check_newer(&rewritten, Some(&current)),
        Err(IndexError::SerialReused(7))
    ));
}

#[test]
fn build_is_selected_by_the_catalog_pins() {
    let index = index::open(&fixture(), &[test_key()]).unwrap();
    let catalog = catalog();
    // Two entries match the catalog pins: the last listed wins. The entry
    // with another core commit never does.
    let build = index::select(
        &index,
        &catalog,
        "azerothcore",
        "playerbot",
        "windows-x86_64",
    )
    .unwrap();
    assert_eq!(build.build_key, "b8a8e89ebf07aeb9");
    let snapshot = build.db_snapshot.as_ref().unwrap();
    assert_eq!(snapshot.mysql.version, "8.4.13");
    assert_eq!(snapshot.file.size, 2000);

    assert!(matches!(
        index::select(&index, &catalog, "azerothcore", "playerbot", "linux-x86_64"),
        Err(IndexError::NoBuild { .. })
    ));
    let mut moved = index.clone();
    for b in &mut moved.builds {
        b.modules.insert("mod-ah-bot".into(), "0".repeat(40));
    }
    assert!(matches!(
        index::select(
            &moved,
            &catalog,
            "azerothcore",
            "playerbot",
            "windows-x86_64"
        ),
        Err(IndexError::NoBuild { .. })
    ));
}

#[test]
fn accepted_index_is_kept_and_rollback_refused_across_runs() {
    use eoa_engine::install::{index::accept_index, Layout};

    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    let keys = [test_key()];
    let older = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/index-test-older.json"),
    )
    .unwrap();

    assert_eq!(accept_index(&layout, &older, &keys).unwrap().serial, 6);
    assert_eq!(accept_index(&layout, &fixture(), &keys).unwrap().serial, 7);
    assert_eq!(
        accept_index(&layout, &fixture(), &keys).unwrap().serial,
        7,
        "same index again"
    );
    let error = accept_index(&layout, &older, &keys).unwrap_err();
    assert!(error.to_string().contains("older"), "{error}");
    // The reference on disk is still the newest one.
    let kept = std::fs::read(dir.path().join("state").join("index.json")).unwrap();
    assert_eq!(kept, fixture());

    // A reference that no longer verifies (key revoked by a launcher update,
    // damaged file) is not a reference any more.
    std::fs::write(dir.path().join("state").join("index.json"), b"{}").unwrap();
    assert_eq!(accept_index(&layout, &older, &keys).unwrap().serial, 6);
}

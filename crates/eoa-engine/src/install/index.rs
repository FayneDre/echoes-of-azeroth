//! Installing from the signed index (ADR 0010): accept an index (signature
//! and anti-rollback against the last accepted one), then download the
//! selected build's artifacts into the store.

use std::path::PathBuf;
use std::sync::Arc;

use eoa_catalog::index::{self, Build, Index, SnapshotArtifact, TrustedKey};
use eoa_core::Catalog;
use eoa_store::{FetchRequest, Store};

use super::{database_component, dbimport_modules, InstanceRecord, Layout, PlanError};
use crate::steps::Fetch;
use crate::{fsutil, Operation};

/// Index documents are small; anything bigger is refused before parsing.
pub const INDEX_SIZE_LIMIT: u64 = 4 * 1024 * 1024;

/// Last accepted index envelope (anti-rollback reference).
fn accepted_path(layout: &Layout) -> PathBuf {
    layout.state().join("index.json")
}

/// Verifies `envelope` with `keys`, refuses it if it is older than (or a
/// rewrite of) the last accepted index, then keeps it as the new reference.
///
/// A stored index that no longer verifies (its key was revoked by a launcher
/// update) is not a reference any more: a revoked key must not be able to
/// pin launchers to its serial.
pub fn accept_index(
    layout: &Layout,
    envelope: &[u8],
    keys: &[TrustedKey],
) -> Result<Index, PlanError> {
    let offered = index::open(envelope, keys)?;
    let path = accepted_path(layout);
    let previous = match std::fs::read(&path) {
        Ok(bytes) => index::open(&bytes, keys).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(crate::EngineError::io(&path)(e).into()),
    };
    index::check_newer(&offered, previous.as_ref())?;
    if previous.as_ref() != Some(&offered) {
        let dir = layout.state();
        std::fs::create_dir_all(&dir).map_err(crate::EngineError::io(&dir))?;
        fsutil::write_atomic(&path, envelope)?;
    }
    Ok(offered)
}

fn request(file: &index::Artifact) -> FetchRequest {
    FetchRequest {
        urls: file.urls.clone(),
        size: file.size,
        sha256: file.sha256.clone(),
    }
}

/// True when the build's snapshot can serve `record` (ADR 0009 rules, checked
/// before downloading it): same MySQL as the catalog, no SQL of a module the
/// instance does not use. Without it the install imports from empty.
pub fn snapshot_usable(
    catalog: &Catalog,
    record: &InstanceRecord,
    snapshot: &SnapshotArtifact,
) -> Result<bool, PlanError> {
    let emu = &catalog
        .emulator(&record.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", record.emulator)))?
        .manifest;
    let mysql = database_component(catalog, emu)?;
    let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
    let resolution = eoa_core::resolve(catalog, &record.emulator, Some(&record.flavor), &selected)?;
    let allowed = dbimport_modules(catalog, &resolution)?;
    Ok(snapshot.mysql.id == mysql.id
        && snapshot.mysql.version == mysql.version
        && snapshot.modules.iter().all(|m| allowed.contains(m)))
}

/// Artifacts of a build, once downloaded: the store blobs to install from.
#[derive(Debug, Clone)]
pub struct BuildFiles {
    pub server: PathBuf,
    pub db_snapshot: Option<PathBuf>,
}

/// `build-<key>`: downloads the server artifact and, when `with_snapshot`,
/// the database snapshot. Shared by every instance of that build.
pub fn build_operation(
    store: Arc<Store>,
    build: &Build,
    with_snapshot: bool,
) -> (Operation<()>, BuildFiles) {
    let mut op = Operation::new(format!("build-{}", build.build_key)).step(Fetch::new(
        "download-server",
        store.clone(),
        request(&build.server),
    ));
    let mut files = BuildFiles {
        server: store.blob_path(&build.server.sha256),
        db_snapshot: None,
    };
    if let Some(snapshot) = build.db_snapshot.as_ref().filter(|_| with_snapshot) {
        files.db_snapshot = Some(store.blob_path(&snapshot.file.sha256));
        op = op.step(Fetch::new(
            "download-db-snapshot",
            store,
            request(&snapshot.file),
        ));
    }
    (op, files)
}

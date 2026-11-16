//! Launcher services (ADR 0014): what a player does, in one place, for the
//! desktop app and the CLI alike. The Tauri layer and `rk` only adapt these
//! calls (arguments in, events and errors out); no business logic lives in
//! them.
//!
//! - [`Launcher`] holds the catalog, the data root and the store. Every
//!   change runs as an engine operation (journal, resume, rollback), with
//!   progress sent to an [`rk_engine::Observer`].
//! - [`session`] runs an instance (and the game) in the background, with
//!   events and console input.
//! - Errors carry a stable code for a translated message ([`UiError`]).

#![forbid(unsafe_code)]

pub mod backups;
pub mod crash;
pub mod diagnostic;
mod error;
pub mod location;
pub mod preferences;
pub mod progress;
pub mod realms;
pub mod session;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rk_client::{ClientLock, ClientState, PrepareReport};
use rk_core::manifest::{ClientManifest, ModuleOption, OptionValue};
use rk_core::Catalog;
use rk_engine::install::{self, InstanceRecord, Layout, Network};
use rk_engine::{CancelToken, Engine, Observer, Operation, Outcome};
use rk_runtime::EmulatorDriver;
use rk_store::Store;
use serde::{Deserialize, Serialize};

pub use error::{LauncherError, UiError};

pub type Result<T, E = LauncherError> = std::result::Result<T, E>;

/// The launcher's own folder: `%LOCALAPPDATA%\RealmKeeperData` (principle 6),
/// or `RK_ROOT` when set (development, tests). Holds the crash reports and
/// the install location; the data root by default.
pub fn app_dir() -> PathBuf {
    if let Some(root) = std::env::var_os("RK_ROOT") {
        return PathBuf::from(root);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_FOLDER)
}

/// Name of the launcher's folder (not `RealmKeeper`: the per-user installer puts
/// the program in `%LOCALAPPDATA%\<product name>`).
const APP_FOLDER: &str = "RealmKeeperData";
/// `<app dir>/location.json`: the data root the player chose (ADR 0017).
const LOCATION_FILE: &str = "location.json";

#[derive(Debug, Serialize, Deserialize)]
struct Location {
    root: PathBuf,
}

/// Marks a data root (ADR 0017): an uninstall removes the launcher's
/// folders in it, so only a folder that is empty or already a data root
/// can become one.
pub const ROOT_MARKER: &str = ".realmkeeper";

const ROOT_MARKER_TEXT: &str = "This folder holds the realms, downloads and backups of \
RealmKeeper. Keep your own files elsewhere: removing a realm deletes the \
launcher's folders here. The folder can be renamed or moved: point the launcher \
at its new place.\r\n";

/// The data root in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    pub root: PathBuf,
    /// The chosen folder no longer exists (renamed, moved, drive
    /// unplugged): `root` is then [`app_dir`] until the player points the
    /// launcher at the new place. Never created again by the launcher.
    pub missing: Option<PathBuf>,
}

/// Where realms, downloads and backups live: the folder the player chose,
/// else [`app_dir`]. `RK_ROOT` always wins.
pub fn data_root() -> DataRoot {
    let app = app_dir();
    if std::env::var_os("RK_ROOT").is_some() {
        return DataRoot {
            root: app,
            missing: None,
        };
    }
    let chosen = std::fs::read(app.join(LOCATION_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Location>(&bytes).ok())
        .map(|l| l.root)
        .filter(|root| root.is_absolute());
    match chosen {
        Some(root) if root.is_dir() => DataRoot {
            root,
            missing: None,
        },
        Some(root) => DataRoot {
            root: app,
            missing: Some(root),
        },
        None => DataRoot {
            root: app,
            missing: None,
        },
    }
}

/// Whether `dir` is a data root: it has the marker, or the folders an
/// earlier launcher made there.
pub fn is_data_root(dir: &Path) -> bool {
    dir.join(ROOT_MARKER).is_file()
        || (dir.join("instances").is_dir() && dir.join("state").is_dir())
}

/// Writes the marker of a data root (best effort).
fn mark_data_root(dir: &Path) {
    let marker = dir.join(ROOT_MARKER);
    if !marker.is_file() {
        let _ = std::fs::write(marker, ROOT_MARKER_TEXT);
    }
}

/// Makes `folder` itself the data root of the next launcher start; returns
/// it. The folder must be empty (or new), or already a data root (a realm
/// folder that was renamed or moved, another installation). Checked
/// writable. Nothing is moved: the caller makes sure the realms of the
/// current root are not left behind.
pub fn set_data_root(folder: &Path) -> Result<PathBuf> {
    if std::env::var_os("RK_ROOT").is_some() {
        return Err(LauncherError::Invalid(
            "RK_ROOT is set: the data root is fixed".into(),
        ));
    }
    let invalid = |reason: String| LauncherError::InvalidLocation {
        path: folder.to_path_buf(),
        reason,
    };
    if !folder.is_absolute() {
        return Err(invalid("not an absolute path".into()));
    }
    let root = folder.to_path_buf();
    let app = app_dir();
    // The launcher's own folder holds its settings: always accepted.
    let has_files = std::fs::read_dir(&root).is_ok_and(|mut entries| entries.next().is_some());
    if root != app && has_files && !is_data_root(&root) {
        return Err(LauncherError::LocationNotEmpty(root));
    }
    std::fs::create_dir_all(&root).map_err(|e| invalid(e.to_string()))?;
    let probe = root.join(".rk-write-test");
    std::fs::write(&probe, b"ok").map_err(|e| invalid(e.to_string()))?;
    let _ = std::fs::remove_file(&probe);
    if root != app {
        location::check(&root)?;
    }
    mark_data_root(&root);

    std::fs::create_dir_all(&app).map_err(|e| io_error(app.clone(), e))?;
    let pointer = app.join(LOCATION_FILE);
    if root == app {
        match std::fs::remove_file(&pointer) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io_error(pointer, e)),
            _ => {}
        }
    } else {
        let tmp = pointer.with_extension("json.tmp");
        let json =
            serde_json::to_vec_pretty(&Location { root: root.clone() }).expect("serializable");
        std::fs::write(&tmp, json).map_err(|e| io_error(tmp.clone(), e))?;
        std::fs::rename(&tmp, &pointer).map_err(|e| io_error(pointer, e))?;
    }
    Ok(root)
}

/// The drivers compiled into this build, by manifest `driver` name.
pub fn driver(name: &str) -> Result<Arc<dyn EmulatorDriver>> {
    match name {
        "azerothcore" => Ok(Arc::new(rk_driver_azerothcore::AzerothCore)),
        other => Err(LauncherError::NoDriver(other.to_owned())),
    }
}

/// A new instance, as the install wizard describes it.
#[derive(Debug, Clone)]
pub struct InstallRequest {
    pub id: String,
    pub emulator: String,
    pub flavor: Option<String>,
    pub modules: Vec<String>,
    pub network: Network,
    /// Local server build ZIP; `None` = the signed index (`index`).
    pub server: Option<PathBuf>,
    /// Database snapshot for `server` (ADR 0009).
    pub db_snapshot: Option<PathBuf>,
    /// Index file or URL; `None` = the published one.
    pub index: Option<String>,
}

/// An installed instance and whether a change of it was interrupted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub record: InstanceRecord,
    /// Interrupted operation, finished by [`Launcher::resume`].
    pub unfinished: Option<String>,
    /// The installation completed once (a server build is recorded). An
    /// install cancelled and rolled back leaves a record without a build:
    /// [`Launcher::resume`] installs it again.
    pub installed: bool,
    /// The data root was renamed or moved since the configs were written:
    /// they point at the old place. [`Launcher::resume`] writes them again.
    pub moved: bool,
}

/// One option of a module with its value in force.
#[derive(Debug, Clone, Serialize)]
pub struct OptionView {
    pub option: ModuleOption,
    pub value: OptionValue,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleOptions {
    pub module: String,
    pub options: Vec<OptionView>,
}

/// One section of realm settings with the values in force.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub id: String,
    pub label: rk_core::LocalizedText,
    pub options: Vec<OptionView>,
}

/// What [`Launcher::uninstall`] did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReport {
    /// The last backup of the player data, kept with the others (restorable
    /// after installing the realm again).
    pub backup: Option<backups::Backup>,
    /// The shared downloads (database engine, server data, caches) went
    /// too: no realm uses them any more.
    pub shared_removed: bool,
    /// Steps that could not be done (the uninstall still completed).
    pub warnings: Vec<String>,
}

/// Disk space a first install of an emulator needs (ADR 0018).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceEstimate {
    /// Bytes still to download (shared parts already there are not).
    pub download: u64,
    /// Bytes the install takes on the disk, downloads included (they stay
    /// in the cache), margin excluded.
    pub needed: u64,
    /// Free bytes on the disk of the data root, when readable.
    pub available: Option<u64>,
    /// The disk is known to be too small (a safety margin included).
    pub too_small: bool,
}

/// `rk update` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum UpdateOutcome {
    UpToDate { build: String },
    Updated { from: Option<String>, to: String },
}

/// The change an unfinished `instance-<id>` operation was started with
/// (`instances/<id>/pending.json`), so that resuming it rebuilds the very
/// same plan (same artifact, same backup name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
enum Pending {
    #[serde(rename_all = "camelCase")]
    Install {
        server: PathBuf,
        db_snapshot: Option<PathBuf>,
    },
    /// Update or repair (`label` names the database dump).
    #[serde(rename_all = "camelCase")]
    Update { server: PathBuf, label: String },
    /// Restore of the player databases `databases` from the backup `dump`.
    #[serde(rename_all = "camelCase")]
    Restore {
        dump: PathBuf,
        databases: Vec<String>,
        label: String,
    },
}

impl Pending {
    fn files_exist(&self) -> bool {
        match self {
            Pending::Install {
                server,
                db_snapshot,
            } => server.is_file() && db_snapshot.as_ref().is_none_or(|s| s.is_file()),
            Pending::Update { server, .. } => server.is_file(),
            Pending::Restore { dump, .. } => dump.is_file(),
        }
    }
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn io_error(path: PathBuf, source: std::io::Error) -> LauncherError {
    LauncherError::Engine(rk_engine::EngineError::Io { path, source })
}

pub struct Launcher {
    catalog: Catalog,
    layout: Layout,
    store: Arc<Store>,
}

impl Launcher {
    pub fn open(catalog_dir: &Path, root: &Path) -> Result<Self> {
        let catalog =
            rk_catalog::load_dir(catalog_dir).map_err(|e| LauncherError::Catalog(e.to_string()))?;
        let issues = rk_catalog::validate(&catalog);
        if !issues.is_empty() {
            let text: Vec<String> = issues.iter().map(ToString::to_string).collect();
            return Err(LauncherError::Catalog(text.join("\n")));
        }
        let layout = Layout::new(root);
        let store = Arc::new(Store::open(layout.store())?);
        mark_data_root(layout.root());
        Ok(Self {
            catalog,
            layout,
            store,
        })
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    fn emulator(&self, id: &str) -> Result<&rk_core::manifest::EmulatorManifest> {
        self.catalog
            .emulator(id)
            .map(|e| &e.manifest)
            .ok_or_else(|| LauncherError::UnknownEmulator(id.to_owned()))
    }

    /// Runs an operation to its end; a finished rollback or a cancellation
    /// is an error (the change did not happen).
    pub async fn run(
        &self,
        op: &Operation<()>,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<()> {
        let engine = Engine::new(self.layout.state());
        match engine.run(op, &(), observer, cancel).await? {
            Outcome::Completed | Outcome::AlreadyCompleted => Ok(()),
            Outcome::RolledBack { failure } => Err(LauncherError::RolledBack {
                operation: op.id().to_owned(),
                failure,
            }),
            Outcome::Cancelled => Err(LauncherError::Cancelled(op.id().to_owned())),
        }
    }

    // ------------------------------------------------------------ instances

    /// Installed instances, sorted by id.
    pub fn instances(&self) -> Result<Vec<InstanceSummary>> {
        let dir = self.layout.instances();
        let mut ids: Vec<String> = match std::fs::read_dir(&dir) {
            // Only realm ids: a copy being built (`.copy-*`) or a removal
            // under way (`.removed-*`) holds an `instance.json` too.
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join("instance.json").is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|id| install::valid_id(id))
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(io_error(dir, e)),
        };
        ids.sort();
        ids.iter().map(|id| self.instance(id)).collect()
    }

    pub fn instance(&self, id: &str) -> Result<InstanceSummary> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let engine = Engine::new(self.layout.state());
        let mut unfinished = None;
        for op in [
            install::instance_operation_id(id),
            install::modules_operation_id(id),
        ] {
            if let Some(journal) = engine.status(&op)? {
                if !journal.status.is_terminal() {
                    unfinished = Some(op);
                }
            }
        }
        // An import whose player data or configs are still to come.
        if unfinished.is_none() && self.import_unfinished(id) {
            unfinished = Some(format!("import-{id}"));
        }
        let installed = record.build.is_some();
        let moved = installed && configs_point_elsewhere(&self.layout.instance(id));
        Ok(InstanceSummary {
            record,
            unfinished,
            installed,
            moved,
        })
    }

    /// Finds the build matching the catalog pins in the signed index and
    /// downloads it (and its database snapshot when wanted and usable).
    async fn fetch_build(
        &self,
        record: &InstanceRecord,
        index: Option<&str>,
        want_snapshot: bool,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<(PathBuf, Option<PathBuf>)> {
        let source = index.unwrap_or(rk_catalog::index::DEFAULT_URL);
        let envelope = if source.starts_with("https://") || source.starts_with("http://") {
            self.store
                .get_document(source, install::index::INDEX_SIZE_LIMIT)
                .await?
        } else {
            std::fs::read(source).map_err(|e| LauncherError::Index(format!("{source}: {e}")))?
        };
        let keys = rk_catalog::index::trusted_keys();
        let accepted = install::index::accept_index(&self.layout, &envelope, &keys)?;
        let build = rk_catalog::index::select(
            &accepted,
            &self.catalog,
            &record.emulator,
            &record.flavor,
            &install::platform(),
        )
        .map_err(|e| LauncherError::Index(e.to_string()))?;
        let with_snapshot = match (&build.db_snapshot, want_snapshot) {
            (Some(snapshot), true) => {
                install::index::snapshot_usable(&self.catalog, record, snapshot)?
            }
            _ => false,
        };
        let (op, files) = install::index::build_operation(self.store.clone(), build, with_snapshot);
        self.run(&op, observer, cancel).await?;
        Ok((files.server, files.db_snapshot))
    }

    // -------------------------------------------------- pending actions

    fn pending_path(&self, id: &str) -> PathBuf {
        self.layout.instance(id).root.join("pending.json")
    }

    /// What the unfinished `instance-<id>` operation was started with.
    fn pending(&self, id: &str) -> Result<Option<Pending>> {
        let path = self.pending_path(id);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| LauncherError::Invalid(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(path, e)),
        }
    }

    fn save_pending(&self, id: &str, pending: &Pending) -> Result<()> {
        let path = self.pending_path(id);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(pending).expect("serializable");
        std::fs::write(&tmp, json).map_err(|e| io_error(tmp.clone(), e))?;
        std::fs::rename(&tmp, &path).map_err(|e| io_error(path, e))
    }

    fn clear_pending(&self, id: &str) -> Result<()> {
        let path = self.pending_path(id);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_error(path, e)),
            _ => Ok(()),
        }
    }

    // ---------------------------------------------------- install / update

    /// What a first install of `emulator` downloads and takes on the disk.
    /// Shared parts already installed (database engine, server data) count
    /// for nothing; the server build itself is checked when it downloads.
    pub fn install_space(&self, emulator: &str) -> Result<SpaceEstimate> {
        let emu = self.emulator(emulator)?;
        let platform = install::platform();
        let (mut download, mut needed) = (0u64, 0u64);
        let mut add = |sha256: &str, size: u64, unpacked: Option<u64>, target: PathBuf| {
            if target.is_dir() {
                return;
            }
            if !self.store.contains(sha256) {
                download += size;
                needed += size;
            }
            needed += unpacked.unwrap_or(size);
        };
        for component in &emu.components {
            if let Some(build) = self
                .catalog
                .components
                .get(&component.id)
                .and_then(|m| m.platforms.get(&platform))
            {
                add(
                    &build.sha256,
                    build.size,
                    build.unpacked_size,
                    self.layout.component(&component.id),
                );
            }
        }
        if let Some(client) = self.catalog.clients.get(&emu.client) {
            let data = &client.server_data;
            add(
                &data.sha256,
                data.size,
                data.unpacked_size,
                self.layout.server_data(&client.id, &data.version),
            );
        }
        needed += emu.instance.disk_size.unwrap_or(0);
        let available = rk_store::available_space(self.layout.root()).ok();
        Ok(SpaceEstimate {
            download,
            needed,
            available,
            too_small: available
                .is_some_and(|a| a < needed.saturating_add(rk_store::space::MARGIN)),
        })
    }

    /// Installs an instance (resumable: the same request again resumes).
    pub async fn install(
        &self,
        request: &InstallRequest,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let emu = self.emulator(&request.emulator)?.clone();
        // A new realm: refuse before downloading gigabytes onto a disk that
        // cannot hold them. A resumed one already took part of its room.
        if !self
            .layout
            .instance(&request.id)
            .root
            .join("instance.json")
            .is_file()
        {
            location::check(self.layout.root())?;
            let space = self.install_space(&request.emulator)?;
            if let (true, Some(available)) = (space.too_small, space.available) {
                return Err(LauncherError::NoSpace(rk_store::NoSpace {
                    path: self.layout.root().to_path_buf(),
                    needed: space.needed.saturating_add(rk_store::space::MARGIN),
                    available,
                }));
            }
        }
        // Shared resources first, each in its own operation.
        for component in &emu.components {
            let op = install::component_operation(
                &self.catalog,
                &self.layout,
                self.store.clone(),
                &component.id,
            )?;
            self.run(&op, observer, cancel).await?;
        }
        let (op, _) = install::server_data_operation(
            &self.catalog,
            &self.layout,
            self.store.clone(),
            &request.emulator,
        )?;
        self.run(&op, observer, cancel).await?;

        let new = install::NewInstance {
            id: request.id.clone(),
            emulator: request.emulator.clone(),
            flavor: request.flavor.clone(),
            modules: request.modules.clone(),
            network: request.network,
        };
        let (record, _) = install::open_or_create(&self.catalog, &self.layout, &new)?;
        let _lock = install::lock_instance(&self.layout, &record.id)?;
        let pending = match (&request.server, self.pending(&record.id)?) {
            (Some(server), _) => Pending::Install {
                server: server.clone(),
                db_snapshot: request.db_snapshot.clone(),
            },
            // A resumed install keeps its files (same plan) while they exist.
            (None, Some(pending @ Pending::Install { .. })) if pending.files_exist() => pending,
            (None, _) => {
                let (server, db_snapshot) = self
                    .fetch_build(&record, request.index.as_deref(), true, observer, cancel)
                    .await?;
                Pending::Install {
                    server,
                    db_snapshot,
                }
            }
        };
        self.save_pending(&record.id, &pending)?;
        self.finish(record, pending, observer, cancel).await
    }

    /// Runs the `instance-<id>` operation described by `pending`, records
    /// the new build, then applies the modules (post-install steps, final
    /// configs). The instance lock is held by the caller.
    async fn finish(
        &self,
        record: InstanceRecord,
        pending: Pending,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let (_, secrets) = install::open_existing(&self.layout, &record.id)?;
        let driver = driver(&self.emulator(&record.emulator)?.driver)?;
        let server = match &pending {
            Pending::Install {
                server,
                db_snapshot,
            } => {
                let op = install::instance_operation(
                    &self.catalog,
                    &self.layout,
                    &record,
                    &secrets,
                    server,
                    db_snapshot.as_deref(),
                )?;
                self.run(&op, observer, cancel).await?;
                Some(server)
            }
            Pending::Restore {
                dump,
                databases,
                label,
            } => {
                let op = rk_runtime::modules::restore_operation(
                    &self.catalog,
                    &self.layout,
                    &record,
                    &secrets,
                    driver.clone(),
                    dump,
                    databases.clone(),
                    label,
                )?;
                self.run(&op, observer, cancel).await?;
                None
            }
            Pending::Update { server, label } => {
                let op = rk_runtime::modules::update_operation(
                    &self.catalog,
                    &self.layout,
                    &record,
                    &secrets,
                    driver.clone(),
                    &self.store,
                    server,
                    label,
                )?;
                self.run(&op, observer, cancel).await?;
                Some(server)
            }
        };
        let build = server.map(|s| install::artifact_build_key(s)).transpose()?;
        if matches!(pending, Pending::Install { .. }) {
            // Part of the install: until it completes, the instance is not
            // installed and `resume` runs both operations again (the first
            // one answers "already completed").
            let op = rk_runtime::modules::apply_modules_operation(
                &self.catalog,
                &self.layout,
                &record,
                &secrets,
                driver,
                &self.store,
            )?;
            self.run(&op, observer, cancel).await?;
        }
        let mut record = record;
        if let Some(build) = build {
            record.build = Some(build);
            install::save_record(&self.layout, &record)?;
        }
        self.clear_pending(&record.id)?;
        // Old automatic dumps go once a new one exists (never manual ones).
        // Best effort: the change itself succeeded, a locked file is
        // pruned next time.
        let _ = backups::prune(&self.layout.backups(&record.id));
        Ok(record)
    }

    /// The pending change with its files present: a store blob that was
    /// cleaned up is fetched again (same content, so the same path and plan).
    async fn with_files(
        &self,
        record: &InstanceRecord,
        pending: Pending,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<Pending> {
        if pending.files_exist() {
            return Ok(pending);
        }
        Ok(match pending {
            Pending::Install { db_snapshot, .. } => {
                let (server, snapshot) = self
                    .fetch_build(record, None, db_snapshot.is_some(), observer, cancel)
                    .await?;
                Pending::Install {
                    server,
                    db_snapshot: snapshot,
                }
            }
            Pending::Update { label, .. } => {
                let (server, _) = self
                    .fetch_build(record, None, false, observer, cancel)
                    .await?;
                Pending::Update { server, label }
            }
            Pending::Restore { dump, .. } => {
                return Err(LauncherError::Invalid(format!(
                    "the backup {} being restored was deleted",
                    dump.display()
                )))
            }
        })
    }

    /// Installs the server build of the current catalog release.
    pub async fn update(
        &self,
        id: &str,
        server: Option<&Path>,
        index: Option<&str>,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<UpdateOutcome> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let _lock = install::lock_instance(&self.layout, id)?;
        let from = record.build.clone();
        let server = match server {
            Some(server) => server.to_path_buf(),
            None => {
                self.fetch_build(&record, index, false, observer, cancel)
                    .await?
                    .0
            }
        };
        let to = install::artifact_build_key(&server)?;
        if from.as_deref() == Some(to.as_str()) {
            return Ok(UpdateOutcome::UpToDate { build: to });
        }
        let label = rk_runtime::modules::update_label(from.as_deref(), &to);
        let pending = Pending::Update { server, label };
        self.save_pending(id, &pending)?;
        self.finish(record, pending, observer, cancel).await?;
        Ok(UpdateOutcome::Updated { from, to })
    }

    /// Reinstalls the server files, SQL tree and configs of the current
    /// release and runs the database update, like an update to the same
    /// build (databases dumped first, rolled back on failure). Fixes deleted
    /// or edited server files; player data is untouched.
    pub async fn repair(
        &self,
        id: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let _lock = install::lock_instance(&self.layout, id)?;
        let (server, _) = self
            .fetch_build(&record, None, false, observer, cancel)
            .await?;
        let to = install::artifact_build_key(&server)?;
        let label = format!("repair-{}-{}", &to[..to.len().min(16)], unix_secs());
        let pending = Pending::Update { server, label };
        self.save_pending(id, &pending)?;
        self.finish(record, pending, observer, cancel).await
    }

    /// Resumes the interrupted change of an instance (install, update,
    /// repair, module or option change, import) with the plan it started with.
    pub async fn resume(
        &self,
        id: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let record = self.resume_change(id, observer, cancel).await?;
        if self.import_unfinished(id) {
            return self.finish_import(id, observer, cancel).await;
        }
        Ok(record)
    }

    async fn resume_change(
        &self,
        id: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let summary = self.instance(id)?;
        let record = summary.record;
        let _lock = install::lock_instance(&self.layout, id)?;
        // An install, update or repair in progress: run it to its end with
        // the inputs it started with (its module operation included).
        if let Some(pending) = self.pending(id)? {
            let pending = self.with_files(&record, pending, observer, cancel).await?;
            self.save_pending(id, &pending)?;
            return self.finish(record, pending, observer, cancel).await;
        }
        match summary.unfinished.as_deref() {
            Some(op) if op == install::modules_operation_id(id) => {
                self.apply(record.clone(), &record, observer, cancel).await
            }
            // The data root was renamed or moved: the module operation
            // writes every config, the server data overlay and the module
            // files again at the new place (its journal is forgotten: the
            // plan may look the same).
            None if summary.moved => {
                Engine::new(self.layout.state()).forget(&install::modules_operation_id(id))?;
                self.apply(record.clone(), &record, observer, cancel).await
            }
            // Finished by `resume` after this (`import.json`).
            Some(op) if op == format!("import-{id}") && summary.installed => Ok(record),
            Some(_) => Err(LauncherError::Invalid(format!(
                "no record of the interrupted change of '{id}'"
            ))),
            // Rolled back before it completed, without its inputs (made by
            // an older launcher): install again from the index.
            None if !summary.installed => {
                let (server, db_snapshot) = self
                    .fetch_build(&record, None, true, observer, cancel)
                    .await?;
                let pending = Pending::Install {
                    server,
                    db_snapshot,
                };
                self.save_pending(id, &pending)?;
                self.finish(record, pending, observer, cancel).await
            }
            None => Ok(record),
        }
    }

    /// Runs `modules-<id>` for a record just saved; puts `old` back if it fails.
    async fn apply(
        &self,
        record: InstanceRecord,
        old: &InstanceRecord,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let (_, secrets) = install::open_existing(&self.layout, &record.id)?;
        let emu = self.emulator(&record.emulator)?;
        let op = rk_runtime::modules::apply_modules_operation(
            &self.catalog,
            &self.layout,
            &record,
            &secrets,
            driver(&emu.driver)?,
            &self.store,
        )?;
        if let Err(error) = self.run(&op, observer, cancel).await {
            install::save_record(&self.layout, old)?;
            return Err(error);
        }
        Ok(rk_runtime::modules::finished(&self.layout, &record)?)
    }

    /// Turns modules on or off (servers stopped). A removed module that
    /// changed the world rebuilds it (ADR 0012): `regenerate` in the
    /// returned record tells, before it starts, via [`Self::plan_modules`].
    pub async fn set_modules(
        &self,
        id: &str,
        enable: &[String],
        disable: &[String],
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let _lock = install::lock_instance(&self.layout, id)?;
        let (record, old) =
            rk_runtime::modules::set_modules(&self.catalog, &self.layout, id, enable, disable)?;
        self.apply(record, &old, observer, cancel).await
    }

    /// Content databases that disabling `disable` would rebuild (to warn
    /// the player that it takes minutes). Changes nothing.
    pub fn rebuilds_for(&self, id: &str, disable: &[String]) -> Result<Vec<String>> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let entry = self
            .catalog
            .emulator(&record.emulator)
            .ok_or_else(|| LauncherError::UnknownEmulator(record.emulator.clone()))?;
        let mut out = Vec::new();
        for module in disable {
            let Some(manifest) = entry.modules.get(module) else {
                continue;
            };
            if !record.modules.contains(module) || manifest.database.apply_when_disabled {
                continue;
            }
            for db in &manifest.database.touches {
                let content = entry
                    .manifest
                    .databases
                    .iter()
                    .any(|d| &d.id == db && !d.player_data);
                if content && !out.contains(db) {
                    out.push(db.clone());
                }
            }
        }
        Ok(out)
    }

    /// Sets option values of one module (`(option, value text)`), applied
    /// like a module change.
    pub async fn set_options(
        &self,
        id: &str,
        module: &str,
        values: &[(String, String)],
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let _lock = install::lock_instance(&self.layout, id)?;
        let (record, old) =
            rk_runtime::modules::set_options(&self.catalog, &self.layout, id, module, values)?;
        self.apply(record, &old, observer, cancel).await
    }

    /// The options of the instance's modules, with the values in force.
    pub fn options(&self, record: &InstanceRecord) -> Vec<ModuleOptions> {
        let Some(entry) = self.catalog.emulator(&record.emulator) else {
            return Vec::new();
        };
        // Mandatory modules have options too (Playerbots).
        let modules = rk_runtime::modules::enabled_modules(&self.catalog, record)
            .unwrap_or_else(|_| record.modules.clone());
        modules
            .iter()
            .filter_map(|module| {
                let manifest = entry.modules.get(module)?;
                let chosen = record.options.get(module);
                let options: Vec<OptionView> = manifest
                    .options
                    .iter()
                    .map(|option| OptionView {
                        value: option.effective(chosen.and_then(|c| c.get(option.id()))),
                        option: option.clone(),
                    })
                    .collect();
                (!options.is_empty()).then(|| ModuleOptions {
                    module: module.clone(),
                    options,
                })
            })
            .collect()
    }

    /// The realm settings (rates, realm type...) with the values in force;
    /// changed with [`Self::set_options`] and the module id
    /// [`rk_core::manifest::REALM_OPTIONS`].
    pub fn realm_settings(&self, record: &InstanceRecord) -> Vec<SettingsView> {
        let Some(entry) = self.catalog.emulator(&record.emulator) else {
            return Vec::new();
        };
        let chosen = record.options.get(rk_core::manifest::REALM_OPTIONS);
        entry
            .manifest
            .settings
            .iter()
            .map(|section| SettingsView {
                id: section.id.clone(),
                label: section.label.clone(),
                options: section
                    .options
                    .iter()
                    .map(|option| OptionView {
                        value: option.effective(chosen.and_then(|c| c.get(option.id()))),
                        option: option.clone(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// The servers of an instance, ready to start. Refused while its
    /// folder was moved and its configs still name the old place, or when
    /// programs it needs are gone (an antivirus quarantine, ADR 0020).
    pub fn server(&self, id: &str) -> Result<rk_runtime::Server> {
        if configs_point_elsewhere(&self.layout.instance(id)) {
            return Err(LauncherError::Moved(id.to_owned()));
        }
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let missing = self.missing_binaries(server.record())?;
        if !missing.is_empty() {
            return Err(LauncherError::FilesMissing {
                instance: id.to_owned(),
                files: missing,
            });
        }
        Ok(server)
    }

    /// Programs and libraries of an instance (its server files, its
    /// components) that are gone or changed since they were installed.
    /// Folders unpacked by an older launcher have no list: their manifest
    /// executables are checked.
    fn missing_binaries(&self, record: &InstanceRecord) -> Result<Vec<String>> {
        let emu = self.emulator(&record.emulator)?;
        let paths = self.layout.instance(&record.id);
        let mut missing = Vec::new();
        let mut check = |dir: &Path, fallback: Vec<PathBuf>| {
            let shown = |name: &str| native(&dir.join(name));
            match rk_engine::binaries::missing(dir) {
                Some(names) => missing.extend(names.iter().map(|n| shown(n))),
                None => missing.extend(
                    fallback
                        .into_iter()
                        .filter(|p| !p.is_file())
                        .map(|p| native(&p)),
                ),
            }
        };
        let executables = emu
            .processes
            .iter()
            .map(|p| paths.root.join(&p.executable))
            .collect();
        check(&paths.bin, executables);
        for component in &emu.components {
            check(&self.layout.component(&component.id), Vec::new());
        }
        Ok(missing)
    }

    // ------------------------------------------------------------- accounts

    pub async fn create_account(&self, id: &str, account: &rk_runtime::NewAccount) -> Result<u32> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.create_account(driver.as_ref(), account).await?)
    }

    /// Player accounts of an instance (its database started for it if the
    /// servers are stopped).
    pub async fn accounts(&self, id: &str) -> Result<Vec<rk_runtime::AccountInfo>> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.accounts(driver.as_ref()).await?.unwrap_or_default())
    }

    /// Changes an account's GM level (applies at its next login).
    pub async fn set_gm_level(&self, id: &str, username: &str, gm_level: u8) -> Result<()> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server
            .set_gm_level(driver.as_ref(), username, gm_level)
            .await?)
    }

    /// Characters in the world (players first), at most `limit`; `None`
    /// while the servers are stopped or when the emulator cannot tell.
    pub async fn online_characters(
        &self,
        id: &str,
        limit: u32,
    ) -> Result<Option<Vec<rk_runtime::OnlineCharacter>>> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.online_characters(driver.as_ref(), limit).await?)
    }

    /// Named teleport places (empty while the servers are stopped).
    pub async fn teleport_locations(&self, id: &str) -> Result<Vec<String>> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.teleport_locations(driver.as_ref()).await?)
    }

    /// The console line of a GM panel action, values checked (ADR 0020).
    pub fn console_action_line(
        &self,
        id: &str,
        action: &str,
        values: &std::collections::BTreeMap<String, String>,
    ) -> Result<String> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let emu = self.emulator(&record.emulator)?;
        let action = emu
            .console_actions
            .iter()
            .find(|a| a.id == action)
            .ok_or_else(|| LauncherError::Invalid(format!("unknown console action '{action}'")))?;
        Ok(rk_core::console::render(action, values)?)
    }

    // -------------------------------------------------------------- backups

    /// Folder of an instance's backups (may not exist yet).
    pub fn backups_dir(&self, id: &str) -> PathBuf {
        self.layout.backups(id)
    }

    /// Backups of an instance, newest first.
    pub fn backups(&self, id: &str) -> Result<Vec<backups::Backup>> {
        install::open_existing(&self.layout, id)?;
        backups::list(&self.layout.backups(id))
    }

    /// Dumps the player databases (characters, accounts, bots) to a new
    /// manual backup: what [`restore`](Self::restore) puts back. Content
    /// databases are left out, the build and modules rebuild them. The
    /// servers must be stopped (the instance lock): the database is
    /// started for the dump and stopped again.
    pub async fn backup_now(&self, id: &str) -> Result<backups::Backup> {
        let _lock = install::lock_instance(&self.layout, id)?;
        self.dump_player_data(id, backups::manual_name(unix_secs()))
            .await
    }

    /// The daily backup (ADR 0018): the player databases, at most once a
    /// day, made when the servers have stopped. `None` when one is less
    /// than a day old or the realm is not fully installed. Busy when the
    /// realm runs or changes (instance lock).
    pub async fn daily_backup(&self, id: &str) -> Result<Option<backups::Backup>> {
        let dir = self.layout.backups(id);
        if !backups::daily_due(&dir, unix_secs())? {
            return Ok(None);
        }
        let summary = self.instance(id)?;
        if !summary.installed || summary.unfinished.is_some() {
            return Ok(None);
        }
        let _lock = install::lock_instance(&self.layout, id)?;
        let backup = self
            .dump_player_data(id, backups::daily_name(unix_secs()))
            .await?;
        // Best effort, like after an update.
        let _ = backups::prune(&dir);
        Ok(Some(backup))
    }

    /// Dumps the player databases to `backups/<id>/<name>`, the instance
    /// lock held by the caller.
    async fn dump_player_data(&self, id: &str, name: String) -> Result<backups::Backup> {
        let dir = self.layout.backups(id);
        std::fs::create_dir_all(&dir).map_err(|e| io_error(dir.clone(), e))?;
        self.dump_player_data_to(id, &dir.join(&name)).await?;
        backups::list(&dir)?
            .into_iter()
            .find(|b| b.name == name)
            .ok_or(LauncherError::BackupNotFound(name))
    }

    /// Dumps the player databases to `out`, the instance lock held by the
    /// caller.
    async fn dump_player_data_to(&self, id: &str, out: &Path) -> Result<()> {
        let (record, secrets) = install::open_existing(&self.layout, id)?;
        install::ensure_finished(
            &self.layout,
            &[
                install::instance_operation_id(id),
                install::modules_operation_id(id),
            ],
        )?;
        let plan = install::instance_plan(&self.catalog, &self.layout, &record, &secrets)?;
        plan.db.start().await?;
        let player = rk_runtime::modules::player_databases(&self.catalog, &record)?;
        let databases: Vec<&str> = player.iter().map(String::as_str).collect();
        let dumped = plan.db.backup(&databases, out).await;
        let stopped = plan.db.stop().await;
        dumped?;
        stopped?;
        Ok(())
    }

    /// Deletes one backup file.
    pub fn delete_backup(&self, id: &str, name: &str) -> Result<()> {
        install::open_existing(&self.layout, id)?;
        let _lock = install::lock_instance(&self.layout, id)?;
        let path = backups::resolve(&self.layout.backups(id), name)?;
        std::fs::remove_file(&path).map_err(|e| io_error(path, e))
    }

    /// Puts the player databases (characters, accounts, bots) back as the
    /// backup `name` holds them; the current state is dumped first. Content
    /// databases are kept, and dbimport migrates the result to the current
    /// build. Resumable like an update.
    pub async fn restore(
        &self,
        id: &str,
        name: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let summary = self.instance(id)?;
        // An interrupted change must be resumed first. A pending.json left
        // by a change that was rolled back is stale: replaced below, as
        // update and repair do.
        if !summary.installed || summary.unfinished.is_some() {
            return Err(install::PlanError::Unfinished(id.to_owned()).into());
        }
        let record = summary.record;
        let _lock = install::lock_instance(&self.layout, id)?;
        let dump = backups::resolve(&self.layout.backups(id), name)?;
        self.restore_locked(record, dump, name, observer, cancel)
            .await
    }

    /// [`Self::restore`] once checked, the instance lock held.
    async fn restore_locked(
        &self,
        record: InstanceRecord,
        dump: PathBuf,
        name: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let id = record.id.clone();
        let id = id.as_str();
        let held = {
            let dump = dump.clone();
            tokio::task::spawn_blocking(move || rk_db::dump_databases(&dump))
                .await
                .map_err(|e| LauncherError::Invalid(e.to_string()))??
        };
        let databases: Vec<String> = rk_runtime::modules::player_databases(&self.catalog, &record)?
            .into_iter()
            .filter(|d| held.contains(d))
            .collect();
        if databases.is_empty() {
            return Err(LauncherError::BackupUnusable(name.to_owned()));
        }
        let pending = Pending::Restore {
            dump,
            databases,
            label: format!("restore-{}", unix_secs()),
        };
        self.save_pending(id, &pending)?;
        self.finish(record, pending, observer, cancel).await
    }

    // ------------------------------------------------------------ uninstall

    /// Removes an installed realm (servers stopped). With `keep_backup`, the
    /// player data is backed up first: backups are never removed here, and
    /// installing the realm again lists them for a restore. Files the
    /// launcher put in the game folder for this realm are removed. Once no
    /// realm is left, the shared downloads go too (database engine, server
    /// data, module files, download cache): a new install fetches them again.
    pub async fn uninstall(&self, id: &str, keep_backup: bool) -> Result<UninstallReport> {
        let summary = self.instance(id)?;
        let record = summary.record;
        let mut warnings = Vec::new();
        let backup = {
            let _lock = install::lock_instance(&self.layout, id)?;
            let backup = if keep_backup && summary.installed && summary.unfinished.is_none() {
                Some(
                    self.dump_player_data(id, backups::manual_name(unix_secs()))
                        .await?,
                )
            } else {
                None
            };
            // The database may still run (a crash, or adopted, ADR 0011):
            // its files could not be deleted.
            if let Ok((_, secrets)) = install::open_existing(&self.layout, id) {
                if let Ok(plan) =
                    install::instance_plan(&self.catalog, &self.layout, &record, &secrets)
                {
                    if let Err(e) = plan.db.stop().await {
                        warnings.push(format!("database: {e}"));
                    }
                }
            }
            if let Err(e) = self.release_client(&record) {
                warnings.push(format!("game folder: {e}"));
            }
            backup
        };

        // Out of the way first (one rename), then deleted: an interrupted
        // delete leaves a folder nobody reads, not a half instance.
        let dir = self.layout.instance(id).root;
        let trash = self
            .layout
            .instances()
            .join(format!(".removed-{id}-{}", unix_secs()));
        std::fs::rename(&dir, &trash).map_err(|e| io_error(dir.clone(), e))?;
        let engine = Engine::new(self.layout.state());
        for op in [
            install::instance_operation_id(id),
            install::modules_operation_id(id),
        ] {
            engine.forget(&op)?;
        }
        if let Err(e) = std::fs::remove_dir_all(&trash) {
            warnings.push(format!("{}: {e}", trash.display()));
        }
        self.remove_trash();

        let shared_removed = self.instances()?.is_empty();
        if shared_removed {
            warnings.extend(self.remove_shared());
        }
        Ok(UninstallReport {
            backup,
            shared_removed,
            warnings,
        })
    }

    /// Removes the files the launcher put in the game folder for `record`
    /// (if the client was last prepared for it).
    fn release_client(&self, record: &InstanceRecord) -> Result<()> {
        let manifest = self.client_manifest_for(&record.emulator)?.clone();
        let dir = self.layout.clients();
        let _lock = rk_client::lock(&dir, &manifest.id)?;
        let Some(mut state) = rk_client::load(&dir, &manifest.id)? else {
            return Ok(());
        };
        if state.last_server.as_deref() == Some(record.id.as_str()) && !state.managed.is_empty() {
            rk_client::prepare(
                &dir,
                &manifest,
                &mut state,
                "127.0.0.1",
                &record.id,
                &[],
                &[],
            )?;
        }
        Ok(())
    }

    /// Leftovers of interrupted uninstalls and duplicates (operations run one
    /// at a time, so none of these is in use when this runs).
    pub(crate) fn remove_trash(&self) {
        let Ok(entries) = std::fs::read_dir(self.layout.instances()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".removed-") || name.starts_with(".copy-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    /// Shared downloads and their journals, once no realm uses them. Kept:
    /// backups, the registered clients, the accepted index (anti-rollback).
    fn remove_shared(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let root = self.layout.instances();
        let root = root.parent().unwrap_or(&root);
        for name in ["components", "serverdata", "assets", "store"] {
            let path = root.join(name);
            match std::fs::remove_dir_all(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    warnings.push(format!("{}: {e}", path.display()))
                }
                _ => {}
            }
        }
        // Their operations must run again: forget them.
        if let Ok(entries) = std::fs::read_dir(self.layout.state()) {
            let engine = Engine::new(self.layout.state());
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(op) = name.strip_suffix(".journal.json") {
                    if let Err(e) = engine.forget(op) {
                        warnings.push(format!("{op}: {e}"));
                    }
                }
            }
        }
        // The open store keeps writing there: its folders come back empty.
        for dir in ["blobs", "partial"] {
            let path = self.layout.store().join(dir);
            if let Err(e) = std::fs::create_dir_all(&path) {
                warnings.push(format!("{}: {e}", path.display()));
            }
        }
        warnings
    }

    // --------------------------------------------------------------- client

    /// The client manifest an emulator needs.
    pub fn client_manifest_for(&self, emulator: &str) -> Result<&ClientManifest> {
        let emu = self.emulator(emulator)?;
        self.catalog
            .clients
            .get(&emu.client)
            .ok_or_else(|| LauncherError::UnknownEmulator(emu.client.clone()))
    }

    pub fn client(&self, client: &str) -> Result<Option<ClientState>> {
        Ok(rk_client::load(&self.layout.clients(), client)?)
    }

    /// Registers the game folder for a client manifest; returns a warning
    /// when a previous folder keeps files the launcher put there.
    pub fn register_client(
        &self,
        client: &str,
        folder: &Path,
        locale: Option<&str>,
    ) -> Result<(ClientState, Option<String>)> {
        let manifest = self
            .catalog
            .clients
            .get(client)
            .ok_or_else(|| LauncherError::UnknownEmulator(client.to_owned()))?;
        let dir = self.layout.clients();
        let _lock = rk_client::lock(&dir, client)?;
        Ok(rk_client::register(&dir, manifest, folder, locale)?)
    }

    /// Points the registered client at an instance and syncs the
    /// instance's client files. The client stays locked by the result.
    pub fn prepare_client(&self, record: &InstanceRecord) -> Result<PreparedClient> {
        let manifest = self.client_manifest_for(&record.emulator)?.clone();
        // A half-applied change would give the client the wrong files.
        install::ensure_finished(
            &self.layout,
            &[
                install::instance_operation_id(&record.id),
                install::modules_operation_id(&record.id),
            ],
        )?;
        let dir = self.layout.clients();
        let lock = rk_client::lock(&dir, &manifest.id)?;
        let mut state = rk_client::load(&dir, &manifest.id)?
            .ok_or_else(|| LauncherError::NoClient(manifest.version.clone()))?;
        let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
        let resolution = rk_core::resolve(
            &self.catalog,
            &record.emulator,
            Some(&record.flavor),
            &selected,
        )
        .map_err(install::PlanError::from)?;
        let assets = install::assets::selected(&self.catalog, &resolution, &record.options)?;
        let files = install::assets::client_files(&self.layout, &assets);
        if let Some((missing, _)) = files.iter().find(|(src, _)| !src.is_file()) {
            return Err(LauncherError::MissingAsset(missing.clone()));
        }
        let addons = install::assets::addon_files(&self.layout, &assets);
        // A selected add-on whose folder is gone or empty: apply again.
        for asset in &assets {
            for addon in &asset.asset.addons {
                let folder = asset.dir(&self.layout).join(&addon.from);
                if !folder.is_dir() {
                    return Err(LauncherError::MissingAsset(folder));
                }
            }
        }
        // The local client always reaches the servers through the loopback.
        let report = rk_client::prepare(
            &dir,
            &manifest,
            &mut state,
            "127.0.0.1",
            &record.id,
            &files,
            &addons,
        )?;
        Ok(PreparedClient {
            manifest,
            state,
            report,
            lock,
        })
    }

    /// Points the registered client of `emulator` at a realm hosted on
    /// another computer of the local network (`address`: IPv4 or host
    /// name). The host's module files are not known here, so the files the
    /// launcher put in the client are removed: a realm with client patches
    /// needs them copied by hand.
    pub fn prepare_client_for_lan(&self, emulator: &str, address: &str) -> Result<PreparedClient> {
        let address = address.trim();
        let valid = !address.is_empty()
            && address.len() <= 253
            && address
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        if !valid {
            return Err(LauncherError::InvalidAddress(address.to_owned()));
        }
        let manifest = self.client_manifest_for(emulator)?.clone();
        let dir = self.layout.clients();
        let lock = rk_client::lock(&dir, &manifest.id)?;
        let mut state = rk_client::load(&dir, &manifest.id)?
            .ok_or_else(|| LauncherError::NoClient(manifest.version.clone()))?;
        let server = format!("lan-{address}");
        let report = rk_client::prepare(&dir, &manifest, &mut state, address, &server, &[], &[])?;
        Ok(PreparedClient {
            manifest,
            state,
            report,
            lock,
        })
    }
}

/// A path as the player's system writes it (manifest and list names use `/`).
fn native(path: &Path) -> String {
    path.display()
        .to_string()
        .replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR)
}

/// Whether the rendered configs of an instance name another place than its
/// folder (the data root was renamed or moved). Configs hold the instance's
/// absolute paths (logs, SQL tree), with `/` or `\\` separators.
fn configs_point_elsewhere(paths: &install::InstancePaths) -> bool {
    let Ok(entries) = std::fs::read_dir(&paths.configs) else {
        return false;
    };
    let here = paths.root.to_string_lossy().to_lowercase();
    let here_slash = here.replace('\\', "/");
    let mut read_any = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        read_any = true;
        let text = text.to_lowercase();
        if text.contains(&here) || text.contains(&here_slash) {
            return false;
        }
    }
    read_any
}

/// A client ready to launch for one instance (locked while held).
pub struct PreparedClient {
    pub manifest: ClientManifest,
    pub state: ClientState,
    pub report: PrepareReport,
    pub lock: ClientLock,
}

/// Option values by module, for convenience in tests and callers.
pub type Options = BTreeMap<String, BTreeMap<String, OptionValue>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>(_: &T) {}

    /// The app runs these futures on Tauri's multi-threaded runtime.
    #[allow(dead_code, unused_must_use)]
    fn futures_are_send(launcher: &Launcher, request: &InstallRequest) {
        let cancel = CancelToken::new();
        assert_send(&launcher.install(request, &(), &cancel));
        assert_send(&launcher.update("x", None, None, &(), &cancel));
        assert_send(&launcher.set_modules("x", &[], &[], &(), &cancel));
        assert_send(&launcher.set_options("x", "m", &[], &(), &cancel));
        assert_send(&launcher.create_account(
            "x",
            &rk_runtime::NewAccount {
                username: String::new(),
                password: String::new(),
                gm_level: 0,
            },
        ));
    }

    #[test]
    fn launcher_opens_the_real_catalog() {
        let root = tempfile::tempdir().unwrap();
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let launcher = Launcher::open(&catalog, root.path()).unwrap();
        assert!(launcher.instances().unwrap().is_empty());
        assert!(launcher.client_manifest_for("azerothcore").is_ok());
        assert!(launcher.client("wow-3-3-5a").unwrap().is_none());
    }

    /// An uninstall removes the instance and, the last one gone, the shared
    /// downloads and their journals; backups and the index stay.
    #[tokio::test]
    async fn uninstall_removes_the_instance_then_the_shared_files() {
        let root = tempfile::tempdir().unwrap();
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let launcher = Launcher::open(&catalog, root.path()).unwrap();
        let layout = launcher.layout().clone();
        for id in ["one", "two"] {
            install::open_or_create(
                launcher.catalog(),
                &layout,
                &install::NewInstance {
                    id: id.into(),
                    emulator: "azerothcore".into(),
                    flavor: None,
                    modules: Vec::new(),
                    network: Network::Solo,
                },
            )
            .unwrap();
        }
        for dir in [
            layout.component("mysql-8-4"),
            layout.server_data("wow-3-3-5a", "v20.0"),
            layout.store().join("blobs"),
            layout.backups("one"),
            layout.state(),
        ] {
            std::fs::create_dir_all(&dir).unwrap();
        }
        std::fs::write(
            layout.state().join("component-mysql-8-4.journal.json"),
            "{}",
        )
        .unwrap();
        std::fs::write(layout.state().join("index.json"), "{}").unwrap();
        std::fs::write(layout.backups("one").join("manual-1.sql"), "--").unwrap();

        let report = launcher.uninstall("one", false).await.unwrap();
        assert!(report.backup.is_none());
        assert!(!report.shared_removed, "'two' still uses them");
        assert!(!layout.instance("one").root.exists());
        assert!(layout.component("mysql-8-4").exists());

        let report = launcher.uninstall("two", true).await.unwrap();
        assert!(
            report.backup.is_none(),
            "never installed: nothing to back up"
        );
        assert!(report.shared_removed);
        assert!(launcher.instances().unwrap().is_empty());
        assert!(!layout.component("mysql-8-4").exists());
        assert!(!layout.server_data("wow-3-3-5a", "v20.0").exists());
        assert!(!layout
            .state()
            .join("component-mysql-8-4.journal.json")
            .exists());
        assert!(layout.state().join("index.json").exists());
        assert!(layout.store().join("blobs").is_dir());
        assert!(layout.backups("one").join("manual-1.sql").exists());
    }

    #[test]
    fn only_an_empty_folder_or_a_data_root_can_become_the_data_root() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("My games");
        std::fs::create_dir_all(mine.join("store")).unwrap();
        assert!(!is_data_root(&mine));
        let error = set_data_root(&mine).unwrap_err();
        assert_eq!(error.to_ui().code, "location.notEmpty");

        // A launcher root (marker written when it opens) is accepted, even
        // renamed.
        let realms = dir.path().join("Realms");
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        Launcher::open(&catalog, &realms).unwrap();
        assert!(is_data_root(&realms));
        let renamed = dir.path().join("Realms (old)");
        std::fs::rename(&realms, &renamed).unwrap();
        assert!(is_data_root(&renamed));
    }

    #[test]
    fn configs_naming_another_folder_mean_a_moved_realm() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        let paths = layout.instance("solo");
        assert!(!configs_point_elsewhere(&paths), "no configs yet");
        std::fs::create_dir_all(&paths.configs).unwrap();
        let here = paths.root.to_string_lossy().replace('\\', "/");
        std::fs::write(
            paths.configs.join("worldserver.conf"),
            format!("LogsDir = \"{here}/logs\"\n"),
        )
        .unwrap();
        assert!(!configs_point_elsewhere(&paths));
        std::fs::write(
            paths.configs.join("worldserver.conf"),
            "LogsDir = \"D:/Old place/instances/solo/logs\"\n",
        )
        .unwrap();
        assert!(configs_point_elsewhere(&paths));
    }

    #[test]
    fn a_first_install_counts_downloads_unpacked_files_and_the_realm() {
        let root = tempfile::tempdir().unwrap();
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let launcher = Launcher::open(&catalog, root.path()).unwrap();
        let space = launcher.install_space("azerothcore").unwrap();
        let mysql = &launcher.catalog().components["mysql-8-4"].platforms[&install::platform()];
        let data = &launcher.catalog().clients["wow-3-3-5a"].server_data;
        assert_eq!(space.download, mysql.size + data.size);
        let disk = launcher.catalog().emulators["azerothcore"]
            .manifest
            .instance
            .disk_size
            .unwrap();
        assert_eq!(
            space.needed,
            space.download + mysql.unpacked_size.unwrap() + data.unpacked_size.unwrap() + disk
        );
        assert!(space.available.is_some());

        // Installed shared parts count for nothing.
        std::fs::create_dir_all(launcher.layout().component("mysql-8-4")).unwrap();
        let after = launcher.install_space("azerothcore").unwrap();
        assert_eq!(after.download, data.size);
    }

    #[test]
    fn a_full_disk_has_its_own_code() {
        let error = LauncherError::Download(rk_store::StoreError::NoSpace(rk_store::NoSpace {
            path: "data".into(),
            needed: 9_100_000_000,
            available: 1_000_000_000,
        }));
        let ui = error.to_ui();
        assert_eq!(ui.code, "disk.full");
        assert_eq!(ui.params["needed"], "9.1");
        assert_eq!(ui.params["available"], "1.0");
        let full = LauncherError::Engine(rk_engine::EngineError::Io {
            path: "x".into(),
            source: std::io::Error::from(std::io::ErrorKind::StorageFull),
        });
        assert_eq!(full.to_ui().code, "disk.fullNow");
    }

    #[test]
    fn errors_have_stable_codes() {
        let error = LauncherError::NoClient("3.3.5a".into());
        let ui = error.to_ui();
        assert_eq!(ui.code, "client.notRegistered");
        assert_eq!(ui.params["version"], "3.3.5a");
    }
}

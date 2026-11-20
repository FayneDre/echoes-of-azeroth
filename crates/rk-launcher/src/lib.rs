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

mod accounts;
pub mod backups;
mod client;
pub mod crash;
pub mod diagnostic;
mod error;
pub mod firewall;
pub mod location;
mod operations;
pub mod overview;
pub mod preferences;
pub mod progress;
mod realm_backups;
mod realm_firewall;
pub mod realms;
pub mod session;
mod uninstall;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rk_client::{ClientLock, ClientState, PrepareReport};
use rk_core::manifest::{ClientManifest, ModuleOption, OptionValue, ServerDataSource};
use rk_core::Catalog;
use rk_engine::install::{self, InstanceRecord, Layout, Network};
use rk_engine::{CancelToken, Engine, Observer, Operation, Outcome};
use rk_runtime::EmulatorDriver;
use rk_store::Store;
use serde::{Deserialize, Serialize};

pub use error::{LauncherError, UiError};

pub type Result<T, E = LauncherError> = std::result::Result<T, E>;

/// Whether emulators marked `experimental` in the catalog are listed and
/// installable (ADR 0024): only with `RK_EXPERIMENTAL=1`, for development.
pub fn experimental_enabled() -> bool {
    std::env::var("RK_EXPERIMENTAL").is_ok_and(|v| v == "1")
}

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
        "pandaria" => Ok(Arc::new(rk_driver_pandaria::Pandaria)),
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
/// What the signed index offers an installed realm (`Launcher::server_update`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum ServerUpdate {
    /// The installed build is the one published for this launcher.
    UpToDate { build: String },
    /// Another build is published: `download` bytes to fetch.
    #[serde(rename_all = "camelCase")]
    Available {
        current: Option<String>,
        build: String,
        download: u64,
    },
    /// Nothing published yet for this launcher's catalog pins.
    NotPublished { current: Option<String> },
}

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
    /// One turn per realm on its database ([`Launcher::database_turn`]).
    turns: std::sync::Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
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
            turns: std::sync::Mutex::new(BTreeMap::new()),
        })
    }

    /// Waits for this process's turn on a realm's database. Held by short
    /// database work run without a session ([`Launcher::on_database`]),
    /// by a daily backup, and by a start or a change until it is
    /// registered: none starts or stops the database under another. Realms
    /// do not wait for each other. (Other processes meet on the instance
    /// lock instead, which refuses rather than waits.)
    pub async fn database_turn(&self, id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let turn = self
            .turns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id.to_owned())
            .or_default()
            .clone();
        turn.lock_owned().await
    }

    /// Short database work on a realm (accounts), in its turn. `route` is
    /// asked once the turn is held: the realm's session, if it has one (the
    /// work then runs through it, so a stop waits for it; refused while the
    /// servers start or stop), or an error to give up (a change in
    /// progress). Without a session the database is borrowed for the work.
    pub async fn on_database<T, E, Fut>(
        self: &Arc<Self>,
        id: &str,
        route: impl FnOnce() -> std::result::Result<Option<session::SessionControl>, E>,
        work: impl FnOnce(Arc<Self>) -> Fut,
    ) -> std::result::Result<T, E>
    where
        T: Send + 'static,
        E: From<LauncherError>,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let turn = self.database_turn(id).await;
        match route()? {
            Some(control) => {
                drop(turn);
                match control.with_database(work(self.clone())).await {
                    Some(done) => Ok(done?),
                    None => Err(LauncherError::NotReady(id.to_owned()).into()),
                }
            }
            None => {
                let done = work(self.clone()).await;
                drop(turn);
                Ok(done?)
            }
        }
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
        let build = self.select_build(record, index).await?.ok_or_else(|| {
            LauncherError::Index(format!(
                "no published server build matches {}/{} on {} with this catalog's pins",
                record.emulator,
                record.flavor,
                install::platform()
            ))
        })?;
        let with_snapshot = match (&build.db_snapshot, want_snapshot) {
            (Some(snapshot), true) => {
                install::index::snapshot_usable(&self.catalog, record, snapshot)?
            }
            _ => false,
        };
        let (op, files) =
            install::index::build_operation(self.store.clone(), &build, with_snapshot);
        self.run(&op, observer, cancel).await?;
        Ok((files.server, files.db_snapshot))
    }

    /// Whether the signed index offers `id` another server build than the
    /// installed one (nothing is downloaded but the index). Builds follow
    /// the catalog pins of this launcher: a newer server comes with a newer
    /// launcher, or is published after it.
    pub async fn server_update(&self, id: &str) -> Result<ServerUpdate> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let Some(build) = self.select_build(&record, None).await? else {
            return Ok(ServerUpdate::NotPublished {
                current: record.build,
            });
        };
        Ok(
            if record.build.as_deref() == Some(build.build_key.as_str()) {
                ServerUpdate::UpToDate {
                    build: build.build_key,
                }
            } else {
                ServerUpdate::Available {
                    current: record.build,
                    build: build.build_key,
                    download: build.server.size,
                }
            },
        )
    }

    /// The build of the signed index matching the catalog pins of `record`;
    /// `None` when none is published for them (yet).
    async fn select_build(
        &self,
        record: &InstanceRecord,
        index: Option<&str>,
    ) -> Result<Option<rk_catalog::index::Build>> {
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
        match rk_catalog::index::select(
            &accepted,
            &self.catalog,
            &record.emulator,
            &record.flavor,
            &install::platform(),
        ) {
            Ok(build) => Ok(Some(build.clone())),
            Err(rk_catalog::index::IndexError::NoBuild { .. }) => Ok(None),
            Err(e) => Err(LauncherError::Index(e.to_string())),
        }
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
    fn unpacked_assets_no_realm_selects_are_removed() {
        let root = tempfile::tempdir().unwrap();
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let launcher = Launcher::open(&catalog, root.path()).unwrap();
        let layout = launcher.layout().clone();
        let module = "mod-individual-progression";
        let (mut record, _) = install::open_or_create(
            launcher.catalog(),
            &layout,
            &install::NewInstance {
                id: "one".into(),
                emulator: "azerothcore".into(),
                flavor: None,
                modules: vec![module.into()],
                network: Network::Solo,
            },
        )
        .unwrap();
        record.build = Some("b".repeat(16));
        record.options.insert(
            module.into(),
            BTreeMap::from([(
                "vanilla-visuals".to_owned(),
                OptionValue::Choice("on".into()),
            )]),
        );
        install::save_record(&layout, &record).unwrap();
        let used = install::assets::selected(
            launcher.catalog(),
            &rk_core::resolve(launcher.catalog(), "azerothcore", None, &[module]).unwrap(),
            &record.options,
        )
        .unwrap()[0]
            .asset
            .source
            .sha256
            .clone();
        let unused = "0".repeat(64);
        let staging = format!("{unused}.rk-staging");
        for name in [&used, &unused, &staging] {
            std::fs::create_dir_all(layout.assets().join(name).join("x")).unwrap();
        }

        // A change in flight: nothing goes.
        std::fs::write(layout.instance("one").root.join("pending.json"), "{}").unwrap();
        assert!(launcher.prune_assets().is_empty());
        std::fs::remove_file(layout.instance("one").root.join("pending.json")).unwrap();

        assert_eq!(launcher.prune_assets(), [layout.assets().join(&unused)]);
        assert!(layout.assets().join(&used).is_dir());
        assert!(
            layout.assets().join(&staging).is_dir(),
            "not an unpacked archive"
        );
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

    // The catalog only ships windows-x64 components.
    #[cfg(windows)]
    #[test]
    fn a_first_install_counts_downloads_unpacked_files_and_the_realm() {
        let root = tempfile::tempdir().unwrap();
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let launcher = Launcher::open(&catalog, root.path()).unwrap();
        let space = launcher.install_space("azerothcore").unwrap();
        let mysql = &launcher.catalog().components["mysql-8-4"].platforms[&install::platform()];
        let data = launcher.catalog().clients["wow-3-3-5a"]
            .server_data
            .as_ref()
            .unwrap();
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

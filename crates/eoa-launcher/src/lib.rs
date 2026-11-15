//! Launcher services (ADR 0014): what a player does, in one place, for the
//! desktop app and the CLI alike. The Tauri layer and `eoa` only adapt these
//! calls (arguments in, events and errors out); no business logic lives in
//! them.
//!
//! - [`Launcher`] holds the catalog, the data root and the store. Every
//!   change runs as an engine operation (journal, resume, rollback), with
//!   progress sent to an [`eoa_engine::Observer`].
//! - [`session`] runs an instance (and the game) in the background, with
//!   events and console input.
//! - Errors carry a stable code for a translated message ([`UiError`]).

#![forbid(unsafe_code)]

pub mod crash;
mod error;
pub mod progress;
pub mod session;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eoa_client::{ClientLock, ClientState, PrepareReport};
use eoa_core::manifest::{ClientManifest, ModuleOption, OptionValue};
use eoa_core::Catalog;
use eoa_engine::install::{self, InstanceRecord, Layout, Network};
use eoa_engine::{CancelToken, Engine, Observer, Operation, Outcome};
use eoa_runtime::EmulatorDriver;
use eoa_store::Store;
use serde::{Deserialize, Serialize};

pub use error::{LauncherError, UiError};

pub type Result<T, E = LauncherError> = std::result::Result<T, E>;

/// `%LOCALAPPDATA%\EchoesOfAzeroth` (principle 6), or `EOA_ROOT` when set
/// (development, tests).
pub fn default_root() -> PathBuf {
    if let Some(root) = std::env::var_os("EOA_ROOT") {
        return PathBuf::from(root);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("EchoesOfAzeroth")
}

/// The drivers compiled into this build, by manifest `driver` name.
pub fn driver(name: &str) -> Result<Arc<dyn EmulatorDriver>> {
    match name {
        "azerothcore" => Ok(Arc::new(eoa_driver_azerothcore::AzerothCore)),
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

/// `eoa update` result.
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
}

impl Pending {
    fn files_exist(&self) -> bool {
        match self {
            Pending::Install {
                server,
                db_snapshot,
            } => server.is_file() && db_snapshot.as_ref().is_none_or(|s| s.is_file()),
            Pending::Update { server, .. } => server.is_file(),
        }
    }
}

fn io_error(path: PathBuf, source: std::io::Error) -> LauncherError {
    LauncherError::Engine(eoa_engine::EngineError::Io { path, source })
}

pub struct Launcher {
    catalog: Catalog,
    layout: Layout,
    store: Arc<Store>,
}

impl Launcher {
    pub fn open(catalog_dir: &Path, root: &Path) -> Result<Self> {
        let catalog = eoa_catalog::load_dir(catalog_dir)
            .map_err(|e| LauncherError::Catalog(e.to_string()))?;
        let issues = eoa_catalog::validate(&catalog);
        if !issues.is_empty() {
            let text: Vec<String> = issues.iter().map(ToString::to_string).collect();
            return Err(LauncherError::Catalog(text.join("\n")));
        }
        let layout = Layout::new(root);
        let store = Arc::new(Store::open(layout.store())?);
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

    fn emulator(&self, id: &str) -> Result<&eoa_core::manifest::EmulatorManifest> {
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
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join("instance.json").is_file())
                .filter_map(|e| e.file_name().into_string().ok())
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
        let installed = record.build.is_some();
        Ok(InstanceSummary {
            record,
            unfinished,
            installed,
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
        let source = index.unwrap_or(eoa_catalog::index::DEFAULT_URL);
        let envelope = if source.starts_with("https://") || source.starts_with("http://") {
            self.store
                .get_document(source, install::index::INDEX_SIZE_LIMIT)
                .await?
        } else {
            std::fs::read(source).map_err(|e| LauncherError::Index(format!("{source}: {e}")))?
        };
        let keys = eoa_catalog::index::trusted_keys();
        let accepted = install::index::accept_index(&self.layout, &envelope, &keys)?;
        let build = eoa_catalog::index::select(
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

    /// Installs an instance (resumable: the same request again resumes).
    pub async fn install(
        &self,
        request: &InstallRequest,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let emu = self.emulator(&request.emulator)?.clone();
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
                server
            }
            Pending::Update { server, label } => {
                let op = eoa_runtime::modules::update_operation(
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
                server
            }
        };
        let build = install::artifact_build_key(server)?;
        if matches!(pending, Pending::Install { .. }) {
            // Part of the install: until it completes, the instance is not
            // installed and `resume` runs both operations again (the first
            // one answers "already completed").
            let op = eoa_runtime::modules::apply_modules_operation(
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
        record.build = Some(build);
        install::save_record(&self.layout, &record)?;
        self.clear_pending(&record.id)?;
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
        let label = eoa_runtime::modules::update_label(from.as_deref(), &to);
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
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let label = format!("repair-{}-{stamp}", &to[..to.len().min(16)]);
        let pending = Pending::Update { server, label };
        self.save_pending(id, &pending)?;
        self.finish(record, pending, observer, cancel).await
    }

    /// Resumes the interrupted change of an instance (install, update,
    /// repair, module or option change) with the plan it started with.
    pub async fn resume(
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
        let op = eoa_runtime::modules::apply_modules_operation(
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
        Ok(eoa_runtime::modules::finished(&self.layout, &record)?)
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
            eoa_runtime::modules::set_modules(&self.catalog, &self.layout, id, enable, disable)?;
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
            eoa_runtime::modules::set_options(&self.catalog, &self.layout, id, module, values)?;
        self.apply(record, &old, observer, cancel).await
    }

    /// The options of the instance's modules, with the values in force.
    pub fn options(&self, record: &InstanceRecord) -> Vec<ModuleOptions> {
        let Some(entry) = self.catalog.emulator(&record.emulator) else {
            return Vec::new();
        };
        record
            .modules
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

    // ------------------------------------------------------------- accounts

    pub async fn create_account(&self, id: &str, account: &eoa_runtime::NewAccount) -> Result<u32> {
        let server = eoa_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.create_account(driver.as_ref(), account).await?)
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
        Ok(eoa_client::load(&self.layout.clients(), client)?)
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
        let _lock = eoa_client::lock(&dir, client)?;
        Ok(eoa_client::register(&dir, manifest, folder, locale)?)
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
        let lock = eoa_client::lock(&dir, &manifest.id)?;
        let mut state = eoa_client::load(&dir, &manifest.id)?
            .ok_or_else(|| LauncherError::NoClient(manifest.version.clone()))?;
        let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
        let resolution = eoa_core::resolve(
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
        // The local client always reaches the servers through the loopback.
        let report =
            eoa_client::prepare(&dir, &manifest, &mut state, "127.0.0.1", &record.id, &files)?;
        Ok(PreparedClient {
            manifest,
            state,
            report,
            lock,
        })
    }
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
            &eoa_runtime::NewAccount {
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

    #[test]
    fn errors_have_stable_codes() {
        let error = LauncherError::NoClient("3.3.5a".into());
        let ui = error.to_ui();
        assert_eq!(ui.code, "client.notRegistered");
        assert_eq!(ui.params["version"], "3.3.5a");
    }
}

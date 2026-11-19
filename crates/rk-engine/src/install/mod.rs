//! Installing an instance from the catalog (ADR 0007).
//!
//! Shared resources (MySQL, server data) are installed by their own
//! operations first, so rolling back an instance never removes something
//! another instance uses. Then one `instance-<id>` operation builds the
//! instance: binaries and SQL from the server artifact, a private MySQL,
//! rendered configs, dbimport.

pub mod assets;
pub mod bindings;
pub mod config;
pub mod index;
mod steps;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rk_core::manifest::{EmulatorManifest, OptionValue, ServerDataSource};
use rk_core::template::TemplateError;
use rk_core::{Catalog, Resolution, ResolveError};
use rk_store::{FetchRequest, Store};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use config::{databases, dbimport_modules, plan_configs, ConfigFilePlan, InstanceValues};
pub use steps::{
    DbAdopt, DbBackup, DbClaim, DbInit, DbProvision, DbRegenerate, DbRestore, DbStart, DbStop,
    RenderConfigs, RunTool, ToolFailed,
};

use crate::steps::{read_entry, Extract, ExtractData, Fetch, Strip};
use crate::{fsutil, Operation};

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("unknown {0}")]
    Unknown(String),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error("invalid instance id '{0}': use 1-64 lowercase letters, digits or '-'")]
    InvalidId(String),
    #[error("no free port for {0}")]
    NoFreePort(String),
    #[error("{path}: {message}")]
    Record { path: PathBuf, message: String },
    #[error(transparent)]
    Engine(#[from] crate::EngineError),
    #[error(transparent)]
    Archive(#[from] crate::steps::ExtractError),
    #[error(transparent)]
    Index(#[from] rk_catalog::index::IndexError),
    #[error("database snapshot {path} does not fit this instance: {reason}")]
    SnapshotMismatch { path: PathBuf, reason: String },
    #[error(
        "instance '{0}' is in use by another command \
         (its server is running, or it is being changed)"
    )]
    InstanceBusy(String),
    #[error(
        "operation '{0}' is unfinished on this instance: run the command \
         that started it again to resume it first"
    )]
    Unfinished(String),
}

/// Where everything lives under the data root (`%LOCALAPPDATA%\RealmKeeperData`).
#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
}

/// Paths of one instance (the root is the servers' working directory).
#[derive(Debug, Clone)]
pub struct InstancePaths {
    pub root: PathBuf,
    pub bin: PathBuf,
    pub sql: PathBuf,
    pub configs: PathBuf,
    pub logs: PathBuf,
    pub db: PathBuf,
    pub record: PathBuf,
    pub secrets: PathBuf,
    /// Results of module post-install steps.
    pub bindings: PathBuf,
    /// Server data overlay, only while a module asset overrides a file.
    pub data: PathBuf,
}

impl Layout {
    /// The root is made absolute: paths end up in `my.ini` and server configs,
    /// where tools resolve relative paths against their own directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            root: std::path::absolute(&root).unwrap_or(root),
        }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn store(&self) -> PathBuf {
        self.root.join("store")
    }
    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    pub fn component(&self, id: &str) -> PathBuf {
        self.root.join("components").join(id)
    }
    pub fn server_data(&self, client: &str, version: &str) -> PathBuf {
        self.root
            .join("serverdata")
            .join(format!("{client}-{version}"))
    }
    /// Database dumps of an instance (kept: they hold player data).
    pub fn backups(&self, id: &str) -> PathBuf {
        self.root.join("backups").join(id)
    }
    /// Unpacked module assets, by archive sha256 (ADR 0013).
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    /// Registered game clients and the files the launcher put in them.
    pub fn clients(&self) -> PathBuf {
        self.root.join("clients")
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn instance(&self, id: &str) -> InstancePaths {
        let root = self.instances().join(id);
        InstancePaths {
            bin: root.join("bin"),
            sql: root.join("sql"),
            configs: root.join("configs"),
            logs: root.join("logs"),
            db: root.join("db"),
            record: root.join("instance.json"),
            secrets: root.join("secrets.json"),
            bindings: root.join("bindings.json"),
            data: root.join("data"),
            root,
        }
    }
}

/// Solo: servers listen on `127.0.0.1` only. LAN: on every interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Network {
    Solo,
    Lan,
}

impl Network {
    pub fn bind(self) -> &'static str {
        match self {
            Network::Solo => "127.0.0.1",
            Network::Lan => "0.0.0.0",
        }
    }
}

/// `instance.json`: what the instance is, fixed at creation (a resumed
/// install uses the same choices, so the same plan).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceRecord {
    pub format: u32,
    pub id: String,
    pub emulator: String,
    pub flavor: String,
    /// Modules chosen by the player (mandatory ones are added by resolution).
    pub modules: Vec<String>,
    pub network: Network,
    pub ports: BTreeMap<String, u16>,
    pub db_port: u16,
    pub created: u64,
    /// Content databases to rebuild at the next module operation (a removed
    /// module changed them). Saved before the operation, so a resumed one
    /// has the same plan; cleared once it completes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regenerate: Vec<String>,
    /// Build key of the installed server (`build-info.json` of the artifact);
    /// `None` for instances made before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// Module option values chosen by the player (module -> option -> value);
    /// absent options use the manifest default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, BTreeMap<String, OptionValue>>,
    /// Name the player gave the realm (a copy, an import), ADR 0020.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `secrets.json`: service account of the instance database. Never journaled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Secrets {
    pub db_user: String,
    pub db_password: String,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, PlanError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| PlanError::Record {
                path: path.to_owned(),
                message: e.to_string(),
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(PlanError::Record {
            path: path.to_owned(),
            message: e.to_string(),
        }),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), PlanError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(crate::EngineError::io(dir))?;
    }
    let json = serde_json::to_vec_pretty(value).expect("plain data serializes");
    Ok(fsutil::write_atomic(path, &json)?)
}

/// What the player asks for when creating an instance.
#[derive(Debug, Clone)]
pub struct NewInstance {
    pub id: String,
    pub emulator: String,
    pub flavor: Option<String>,
    pub modules: Vec<String>,
    pub network: Network,
}

/// An instance id: 1 to 64 lowercase letters, digits and dashes (a folder
/// name and part of operation ids).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Loads the instance record, or creates it (ports, database password) on the
/// first call. A resumed install gets the very same record back.
pub fn open_or_create(
    catalog: &Catalog,
    layout: &Layout,
    request: &NewInstance,
) -> Result<(InstanceRecord, Secrets), PlanError> {
    if !valid_id(&request.id) {
        return Err(PlanError::InvalidId(request.id.clone()));
    }
    let paths = layout.instance(&request.id);
    if let (Some(record), Some(secrets)) = (
        read_json::<InstanceRecord>(&paths.record)?,
        read_json::<Secrets>(&paths.secrets)?,
    ) {
        return Ok((record, secrets));
    }
    let selected: Vec<&str> = request.modules.iter().map(String::as_str).collect();
    let resolution = rk_core::resolve(
        catalog,
        &request.emulator,
        request.flavor.as_deref(),
        &selected,
    )?;
    let emu = &catalog
        .emulator(&request.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", request.emulator)))?
        .manifest;
    let db_port = allocate_db_port(layout)?;
    let record = InstanceRecord {
        format: 1,
        id: request.id.clone(),
        emulator: request.emulator.clone(),
        flavor: resolution.flavor,
        modules: request.modules.clone(),
        network: request.network,
        ports: emu.ports.clone(),
        db_port,
        created: crate::journal::now(),
        regenerate: Vec::new(),
        build: None,
        options: BTreeMap::new(),
        name: None,
    };
    let secrets = Secrets {
        db_user: "rk".into(),
        db_password: rk_db::generate_password(32).map_err(|e| PlanError::Record {
            path: paths.secrets.clone(),
            message: e.to_string(),
        })?,
    };
    // Secrets first: a record without them would be unusable.
    write_json(&paths.secrets, &secrets)?;
    write_json(&paths.record, &record)?;
    Ok((record, secrets))
}

/// Loads an existing instance's record and secrets.
pub fn open_existing(layout: &Layout, id: &str) -> Result<(InstanceRecord, Secrets), PlanError> {
    let paths = layout.instance(id);
    match (
        read_json::<InstanceRecord>(&paths.record)?,
        read_json::<Secrets>(&paths.secrets)?,
    ) {
        (Some(record), Some(secrets)) => Ok((record, secrets)),
        _ => Err(PlanError::Unknown(format!("instance '{id}'"))),
    }
}

/// The MySQL server of an instance (not started).
pub fn database(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
) -> Result<rk_db::Instance, PlanError> {
    let emu = &catalog
        .emulator(&record.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", record.emulator)))?
        .manifest;
    let mysql = database_component(catalog, emu)?;
    Ok(rk_db::Instance::new(
        layout.instance(&record.id).db,
        rk_db::Binaries::from_basedir(layout.component(&mysql.id)),
        rk_db::ServerSettings {
            port: record.db_port,
            ..Default::default()
        },
    ))
}

/// A database port no instance uses and nothing listens on.
pub fn allocate_db_port(layout: &Layout) -> Result<u16, PlanError> {
    let used = used_db_ports(layout)?;
    (3307..3400)
        .find(|p| !used.contains(p) && port_is_free(*p))
        .ok_or_else(|| PlanError::NoFreePort("the instance database".into()))
}

fn used_db_ports(layout: &Layout) -> Result<Vec<u16>, PlanError> {
    let mut ports = Vec::new();
    let Ok(entries) = std::fs::read_dir(layout.instances()) else {
        return Ok(ports);
    };
    // Realm folders only: not a copy being built or a removal under way.
    for entry in entries
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(valid_id))
    {
        if let Some(record) = read_json::<InstanceRecord>(&entry.path().join("instance.json"))? {
            ports.push(record.db_port);
        }
    }
    Ok(ports)
}

fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Catalog platform key of this machine (`windows-x86_64`).
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The database component of an emulator and its installed directory.
fn database_component<'a>(
    catalog: &'a Catalog,
    emu: &EmulatorManifest,
) -> Result<&'a rk_core::manifest::ComponentManifest, PlanError> {
    emu.components
        .iter()
        .filter_map(|c| catalog.components.get(&c.id))
        .find(|c| matches!(c.kind, rk_core::manifest::ComponentKind::Database))
        .ok_or_else(|| PlanError::Unknown(format!("database component of '{}'", emu.id)))
}

/// `component-<id>`: downloads and unpacks a shared component.
pub fn component_operation(
    catalog: &Catalog,
    layout: &Layout,
    store: Arc<Store>,
    id: &str,
) -> Result<Operation<()>, PlanError> {
    let component = catalog
        .components
        .get(id)
        .ok_or_else(|| PlanError::Unknown(format!("component '{id}'")))?;
    let key = platform();
    let build = component
        .platforms
        .get(&key)
        .ok_or_else(|| PlanError::Unknown(format!("build of '{id}' for {key}")))?;
    let blob = store.blob_path(&build.sha256);
    let request = FetchRequest {
        urls: vec![build.upstream.clone()],
        size: build.size,
        sha256: build.sha256.clone(),
    };
    Ok(Operation::new(format!("component-{id}"))
        .step(Fetch::new("download", store, request))
        .step(Extract::new("extract", blob, layout.component(id)).strip(Strip::SingleRootDir)))
}

/// `serverdata-<client>-<version>`: the emulator's downloaded server data
/// (ADR 0006). `None` when the emulator extracts it from the player's client
/// instead: [`extraction_operation`], once a realm holds the tools.
pub fn server_data_operation(
    catalog: &Catalog,
    layout: &Layout,
    store: Arc<Store>,
    emulator: &str,
) -> Result<Option<(Operation<()>, PathBuf)>, PlanError> {
    let emu = &catalog
        .emulator(emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{emulator}'")))?
        .manifest;
    let client = catalog
        .clients
        .get(&emu.client)
        .ok_or_else(|| PlanError::Unknown(format!("client '{}'", emu.client)))?;
    let source = catalog
        .server_data_source(emulator)
        .ok_or_else(|| PlanError::Unknown(format!("server data of '{emulator}'")))?;
    let data = match source {
        ServerDataSource::Download(data) => data,
        ServerDataSource::Extract(_) => return Ok(None),
    };
    let target = layout.server_data(&client.id, &source.folder(emulator));
    let blob = store.blob_path(&data.sha256);
    let request = FetchRequest {
        urls: vec![data.url.clone()],
        size: data.size,
        sha256: data.sha256.clone(),
    };
    let op = Operation::new(format!("serverdata-{}-{}", client.id, data.version))
        .step(Fetch::new("download", store, request))
        .step(Extract::new("extract", blob, &target));
    Ok(Some((op, target)))
}

/// What an extraction of server data reads (ADR 0024).
#[derive(Debug, Clone)]
pub struct ExtractionInput {
    /// Root of an installed realm of the emulator: its build has the tools.
    pub tools_root: PathBuf,
    /// The player's registered game folder.
    pub client_root: PathBuf,
    pub logs: PathBuf,
    pub cpus: usize,
}

/// `serverdata-<client>-extract-<emulator>-<version>`: the emulator's server
/// data made from the player's client (ADR 0024). `None` when the emulator
/// downloads it ([`server_data_operation`]).
pub fn extraction_operation(
    catalog: &Catalog,
    layout: &Layout,
    emulator: &str,
    input: ExtractionInput,
) -> Result<Option<(Operation<()>, PathBuf)>, PlanError> {
    let emu = &catalog
        .emulator(emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{emulator}'")))?
        .manifest;
    let source = catalog
        .server_data_source(emulator)
        .ok_or_else(|| PlanError::Unknown(format!("server data of '{emulator}'")))?;
    let ServerDataSource::Extract(extraction) = source else {
        return Ok(None);
    };
    let folder = source.folder(emulator);
    let target = layout.server_data(&emu.client, &folder);
    let op = Operation::new(format!("serverdata-{}-{folder}", emu.client)).step(ExtractData {
        id: "extract-data".into(),
        extraction: extraction.clone(),
        tools_root: input.tools_root,
        client_root: input.client_root,
        target: target.clone(),
        logs: input.logs,
        cpus: input.cpus,
    });
    Ok(Some((op, target)))
}

/// `snapshot.json` of a database snapshot archive (ADR 0009).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub format: u32,
    /// Build key of the server artifact whose SQL made the snapshot.
    pub build_key: String,
    pub mysql: SnapshotMysql,
    pub databases: Vec<String>,
    /// Modules whose SQL is in the snapshot (dbimport names).
    pub modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMysql {
    pub id: String,
    pub version: String,
}

fn read_manifest<T: for<'de> Deserialize<'de>>(archive: &Path, name: &str) -> Result<T, PlanError> {
    let bytes = read_entry(archive, name)?.ok_or_else(|| PlanError::Record {
        path: archive.join(name),
        message: "missing from the archive".into(),
    })?;
    serde_json::from_slice(&bytes).map_err(|e| PlanError::Record {
        path: archive.join(name),
        message: e.to_string(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildInfo {
    build_key: String,
}

/// Build key of a server artifact (its `build-info.json`).
pub fn artifact_build_key(server_artifact: &Path) -> Result<String, PlanError> {
    let build: BuildInfo = read_manifest(server_artifact, "build-info.json")?;
    Ok(build.build_key)
}

/// A snapshot fits when it was made from the same server build and MySQL
/// version, and holds no SQL of a module the instance does not allow.
fn check_snapshot(
    snapshot: &Path,
    server_artifact: &Path,
    mysql: (&str, &str),
    allowed: &[String],
) -> Result<(), PlanError> {
    let manifest: SnapshotManifest = read_manifest(snapshot, "snapshot.json")?;
    let build = BuildInfo {
        build_key: artifact_build_key(server_artifact)?,
    };
    let mismatch = |reason: String| PlanError::SnapshotMismatch {
        path: snapshot.to_owned(),
        reason,
    };
    if manifest.format != 1 {
        return Err(mismatch(format!("unknown format {}", manifest.format)));
    }
    if manifest.build_key != build.build_key {
        return Err(mismatch(format!(
            "made for server build {}, not {}",
            manifest.build_key, build.build_key
        )));
    }
    let (mysql_id, mysql_version) = mysql;
    if manifest.mysql.id != mysql_id || manifest.mysql.version != mysql_version {
        return Err(mismatch(format!(
            "made with {} {}, not {mysql_id} {mysql_version}",
            manifest.mysql.id, manifest.mysql.version
        )));
    }
    if let Some(extra) = manifest.modules.iter().find(|m| !allowed.contains(m)) {
        return Err(mismatch(format!(
            "contains the SQL of module '{extra}', which this instance does not use"
        )));
    }
    Ok(())
}

/// What every operation on an existing instance needs: its resolution, its
/// database and the steps that (re)render its configs and run dbimport.
pub struct InstancePlan {
    pub resolution: Resolution,
    pub db: Arc<rk_db::Instance>,
    /// Database component (id, version).
    pub mysql: (String, String),
    pub databases: Vec<String>,
    pub render: RenderConfigs,
    /// dbimport, when the emulator migrates through one (else the servers
    /// migrate their databases themselves at start).
    pub import: Option<RunTool>,
    /// Module assets selected by the option values (ADR 0013).
    pub assets: Vec<assets::SelectedAsset>,
    /// Shared server data and the directory `DataDir` points at (the
    /// instance overlay while a server override exists).
    pub server_data: PathBuf,
    pub data: PathBuf,
}

/// Plans the configs and dbimport of an instance from the catalog and its
/// record (modules included). No I/O.
pub fn instance_plan(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    secrets: &Secrets,
) -> Result<InstancePlan, PlanError> {
    let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
    let resolution: Resolution =
        rk_core::resolve(catalog, &record.emulator, Some(&record.flavor), &selected)?;
    let emu = &catalog
        .emulator(&record.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", record.emulator)))?
        .manifest;
    let client = catalog
        .clients
        .get(&emu.client)
        .ok_or_else(|| PlanError::Unknown(format!("client '{}'", emu.client)))?;
    let paths = layout.instance(&record.id);

    let mysql = database_component(catalog, emu)?;
    let mysql_dir = layout.component(&mysql.id);
    let mysql_client = mysql
        .platforms
        .get(&platform())
        .and_then(|b| b.executables.get("client"))
        .map(|rel| mysql_dir.join(rel))
        .ok_or_else(|| PlanError::Unknown(format!("client executable of '{}'", mysql.id)))?;
    let db = Arc::new(database(catalog, layout, record)?);
    let selected_assets = assets::selected(catalog, &resolution, &record.options)?;
    let source = catalog
        .server_data_source(&record.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("server data of '{}'", record.emulator)))?;
    let server_data = layout.server_data(&client.id, &source.folder(&record.emulator));
    let data = if selected_assets.iter().any(|a| !a.asset.server.is_empty()) {
        paths.data.clone()
    } else {
        server_data.clone()
    };

    let values = InstanceValues {
        root: &paths.root,
        logs: &paths.logs,
        sql: &paths.sql,
        data: &data,
        mysql_client: &mysql_client,
        ports: &record.ports,
        bind: record.network.bind(),
        db_port: record.db_port,
        db_user: &secrets.db_user,
        db_password: &secrets.db_password,
        options: &record.options,
    };
    let files = plan_configs(catalog, &resolution, &values)?;
    let databases = databases(catalog, &resolution)?
        .into_iter()
        .map(|d| d.name)
        .collect();
    Ok(InstancePlan {
        db,
        mysql: (mysql.id.clone(), mysql.version.clone()),
        databases,
        render: RenderConfigs {
            dist_dir: paths.bin.join("configs"),
            target: paths.configs.clone(),
            files,
            bindings: Some(paths.bindings.clone()),
        },
        import: emu.instance.db_import.as_ref().map(|tool| RunTool {
            id: "db-import".into(),
            executable: paths.root.join(&tool.executable),
            args: vec!["-c".into(), format!("configs/{}", tool.config_file)],
            cwd: paths.root.clone(),
            log: paths.logs.join("dbimport.log"),
        }),
        resolution,
        assets: selected_assets,
        server_data,
        data,
    })
}

/// `instance-<id>`: builds the instance from the server artifact (a ZIP with
/// `dist/` and `sql/`, as produced by CI). With a database snapshot (ADR
/// 0009), the database is restored from it instead of being initialised
/// empty; dbimport then only applies what the snapshot lacks.
pub fn instance_operation(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    secrets: &Secrets,
    server_artifact: &Path,
    db_snapshot: Option<&Path>,
) -> Result<Operation<()>, PlanError> {
    let plan = instance_plan(catalog, layout, record, secrets)?;
    let paths = layout.instance(&record.id);
    let db = plan.db;
    let op = Operation::new(instance_operation_id(&record.id))
        .step(Extract::new("extract-bin", server_artifact, &paths.bin).subdir("dist"))
        .step(Extract::new("extract-sql", server_artifact, &paths.sql).subdir("sql"));
    let op = match db_snapshot {
        Some(snapshot) => {
            check_snapshot(
                snapshot,
                server_artifact,
                (&plan.mysql.0, &plan.mysql.1),
                &dbimport_modules(catalog, &plan.resolution)?,
            )?;
            op.step(DbClaim { db: db.clone() })
                .step(Extract::new("extract-db", snapshot, db.datadir()).subdir("data"))
                .step(DbAdopt { db: db.clone() })
        }
        None => op
            .step(DbInit { db: db.clone() })
            .step(DbStart { db: db.clone() }),
    };
    Ok(op
        .step(DbProvision {
            db: db.clone(),
            databases: plan.databases,
            user: secrets.db_user.clone(),
            password: secrets.db_password.clone(),
        })
        .step(plan.render)
        .maybe_step(plan.import)
        .step(DbStop { db }))
}

/// Writes `instance.json` after a change of modules (atomic).
pub fn save_record(layout: &Layout, record: &InstanceRecord) -> Result<(), PlanError> {
    write_json(&layout.instance(&record.id).record, record)
}

/// Exclusive OS lock on one instance, released when dropped (also on crash).
/// Held while its servers run and while it is changed, so that a module
/// change or an update never runs under a server that is still loading.
#[derive(Debug)]
pub struct InstanceLock {
    _file: std::fs::File,
}

pub fn lock_instance(layout: &Layout, id: &str) -> Result<InstanceLock, PlanError> {
    let path = layout.instance(id).root.join("instance.lock");
    let io = |e: std::io::Error| crate::EngineError::Io {
        path: path.clone(),
        source: e,
    };
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(io)?;
    match file.try_lock() {
        Ok(()) => Ok(InstanceLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(PlanError::InstanceBusy(id.to_owned())),
        Err(std::fs::TryLockError::Error(e)) => Err(io(e).into()),
    }
}

/// Journal ids of the operations that change an installed instance.
pub fn instance_operation_id(id: &str) -> String {
    format!("instance-{id}")
}
pub fn modules_operation_id(id: &str) -> String {
    format!("modules-{id}")
}

/// Fails when one of `operations` was interrupted (running, rolling back,
/// or needing repair): the instance is half-changed until it is resumed.
pub fn ensure_finished(layout: &Layout, operations: &[String]) -> Result<(), PlanError> {
    let engine = crate::Engine::new(layout.state());
    for operation in operations {
        if let Some(journal) = engine.status(operation)? {
            if !journal.status.is_terminal() {
                return Err(PlanError::Unfinished(operation.clone()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn archive(path: &Path, name: &str, json: &str) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(json.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    fn check(snapshot_json: &str, allowed: &[&str]) -> Result<(), PlanError> {
        let dir = tempfile::tempdir().unwrap();
        let (snapshot, server) = (dir.path().join("db.zip"), dir.path().join("server.zip"));
        archive(&snapshot, "snapshot.json", snapshot_json);
        archive(
            &server,
            "build-info.json",
            r#"{"buildKey": "k1", "other": 1}"#,
        );
        let allowed: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
        check_snapshot(&snapshot, &server, ("mysql-8-4", "8.4.13"), &allowed)
    }

    fn manifest(build_key: &str, version: &str, modules: &str) -> String {
        format!(
            r#"{{"format": 1, "buildKey": "{build_key}",
                "mysql": {{"id": "mysql-8-4", "version": "{version}", "sha256": "ab"}},
                "databases": ["acore_world"], "modules": [{modules}]}}"#
        )
    }

    #[test]
    fn matching_snapshot_is_accepted() {
        let json = manifest("k1", "8.4.13", r#""mod-playerbots", "mod-ah-bot""#);
        check(
            &json,
            &["mod-playerbots", "mod-ah-bot", "mod-individual-progression"],
        )
        .unwrap();
    }

    #[test]
    fn other_build_mysql_or_module_is_refused() {
        let cases = [
            (
                manifest("k2", "8.4.13", r#""mod-playerbots""#),
                "server build k2",
            ),
            (manifest("k1", "8.4.14", r#""mod-playerbots""#), "8.4.14"),
            (
                manifest("k1", "8.4.13", r#""mod-individual-progression""#),
                "mod-individual-progression",
            ),
        ];
        for (json, expected) in cases {
            let error = check(&json, &["mod-playerbots"]).unwrap_err();
            assert!(
                matches!(error, PlanError::SnapshotMismatch { .. }),
                "{error}"
            );
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[test]
    fn archive_without_manifest_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (snapshot, server) = (dir.path().join("db.zip"), dir.path().join("server.zip"));
        archive(&snapshot, "data/mysql.ibd", "");
        archive(&server, "build-info.json", r#"{"buildKey": "k1"}"#);
        let error = check_snapshot(&snapshot, &server, ("mysql-8-4", "8.4.13"), &[]).unwrap_err();
        assert!(error.to_string().contains("snapshot.json"), "{error}");
    }
}

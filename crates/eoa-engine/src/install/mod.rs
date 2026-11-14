//! Installing an instance from the catalog (ADR 0007).
//!
//! Shared resources (MySQL, server data) are installed by their own
//! operations first, so rolling back an instance never removes something
//! another instance uses. Then one `instance-<id>` operation builds the
//! instance: binaries and SQL from the server artifact, a private MySQL,
//! rendered configs, dbimport.

pub mod config;
mod steps;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eoa_core::manifest::EmulatorManifest;
use eoa_core::template::TemplateError;
use eoa_core::{Catalog, Resolution, ResolveError};
use eoa_store::{FetchRequest, Store};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use config::{databases, plan_configs, ConfigFilePlan, InstanceValues};
pub use steps::{DbInit, DbProvision, DbStart, DbStop, RenderConfigs, RunTool, ToolFailed};

use crate::steps::{Extract, Fetch, Strip};
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
}

/// Where everything lives under the data root (`%LOCALAPPDATA%\EchoesOfAzeroth`).
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Loads the instance record, or creates it (ports, database password) on the
/// first call. A resumed install gets the very same record back.
pub fn open_or_create(
    catalog: &Catalog,
    layout: &Layout,
    request: &NewInstance,
) -> Result<(InstanceRecord, Secrets), PlanError> {
    let id_ok = !request.id.is_empty()
        && request.id.len() <= 64
        && request
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
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
    let resolution = eoa_core::resolve(
        catalog,
        &request.emulator,
        request.flavor.as_deref(),
        &selected,
    )?;
    let emu = &catalog
        .emulator(&request.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", request.emulator)))?
        .manifest;
    let used = used_db_ports(layout)?;
    let db_port = (3307..3400)
        .find(|p| !used.contains(p) && port_is_free(*p))
        .ok_or_else(|| PlanError::NoFreePort("the instance database".into()))?;
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
    };
    let secrets = Secrets {
        db_user: "eoa".into(),
        db_password: eoa_db::generate_password(32).map_err(|e| PlanError::Record {
            path: paths.secrets.clone(),
            message: e.to_string(),
        })?,
    };
    // Secrets first: a record without them would be unusable.
    write_json(&paths.secrets, &secrets)?;
    write_json(&paths.record, &record)?;
    Ok((record, secrets))
}

fn used_db_ports(layout: &Layout) -> Result<Vec<u16>, PlanError> {
    let mut ports = Vec::new();
    let Ok(entries) = std::fs::read_dir(layout.instances()) else {
        return Ok(ports);
    };
    for entry in entries.flatten() {
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
) -> Result<&'a eoa_core::manifest::ComponentManifest, PlanError> {
    emu.components
        .iter()
        .filter_map(|c| catalog.components.get(&c.id))
        .find(|c| matches!(c.kind, eoa_core::manifest::ComponentKind::Database))
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

/// `serverdata-<client>-<version>`: the emulator's server data (ADR 0006).
pub fn server_data_operation(
    catalog: &Catalog,
    layout: &Layout,
    store: Arc<Store>,
    emulator: &str,
) -> Result<(Operation<()>, PathBuf), PlanError> {
    let emu = &catalog
        .emulator(emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{emulator}'")))?
        .manifest;
    let client = catalog
        .clients
        .get(&emu.client)
        .ok_or_else(|| PlanError::Unknown(format!("client '{}'", emu.client)))?;
    let data = &client.server_data;
    let target = layout.server_data(&client.id, &data.version);
    let blob = store.blob_path(&data.sha256);
    let request = FetchRequest {
        urls: vec![data.url.clone()],
        size: data.size,
        sha256: data.sha256.clone(),
    };
    let op = Operation::new(format!("serverdata-{}-{}", client.id, data.version))
        .step(Fetch::new("download", store, request))
        .step(Extract::new("extract", blob, &target));
    Ok((op, target))
}

/// `instance-<id>`: builds the instance from the server artifact (a ZIP with
/// `dist/` and `sql/`, as produced by CI).
pub fn instance_operation(
    catalog: &Catalog,
    layout: &Layout,
    record: &InstanceRecord,
    secrets: &Secrets,
    server_artifact: &Path,
) -> Result<Operation<()>, PlanError> {
    let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
    let resolution: Resolution =
        eoa_core::resolve(catalog, &record.emulator, Some(&record.flavor), &selected)?;
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
    let db = Arc::new(eoa_db::Instance::new(
        &paths.db,
        eoa_db::Binaries::from_basedir(&mysql_dir),
        eoa_db::ServerSettings {
            port: record.db_port,
            ..Default::default()
        },
    ));

    let values = InstanceValues {
        root: &paths.root,
        logs: &paths.logs,
        sql: &paths.sql,
        data: &layout.server_data(&client.id, &client.server_data.version),
        mysql_client: &mysql_client,
        ports: &record.ports,
        bind: record.network.bind(),
        db_port: record.db_port,
        db_user: &secrets.db_user,
        db_password: &secrets.db_password,
    };
    let files = plan_configs(catalog, &resolution, &values)?;
    let database_names = databases(catalog, &resolution)?
        .into_iter()
        .map(|d| d.name)
        .collect();
    let tool = &emu.instance.db_import;

    Ok(Operation::new(format!("instance-{}", record.id))
        .step(Extract::new("extract-bin", server_artifact, &paths.bin).subdir("dist"))
        .step(Extract::new("extract-sql", server_artifact, &paths.sql).subdir("sql"))
        .step(DbInit { db: db.clone() })
        .step(DbStart { db: db.clone() })
        .step(DbProvision {
            db: db.clone(),
            databases: database_names,
            user: secrets.db_user.clone(),
            password: secrets.db_password.clone(),
        })
        .step(RenderConfigs {
            dist_dir: paths.bin.join("configs"),
            target: paths.configs.clone(),
            files,
        })
        .step(RunTool {
            id: "db-import".into(),
            executable: paths.root.join(&tool.executable),
            args: vec!["-c".into(), format!("configs/{}", tool.config_file)],
            cwd: paths.root.clone(),
            log: paths.logs.join("dbimport.log"),
        })
        .step(DbStop { db }))
}

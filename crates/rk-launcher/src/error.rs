use std::collections::BTreeMap;

use rk_client::ClientError;
use rk_engine::install::PlanError;
use rk_engine::EngineError;
use rk_runtime::RuntimeError;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("the catalog cannot be loaded: {0}")]
    Catalog(String),
    #[error("unknown emulator '{0}'")]
    UnknownEmulator(String),
    #[error("no driver '{0}' in this build")]
    NoDriver(String),
    #[error("no {0} client registered: choose your game folder first")]
    NoClient(String),
    #[error("{0} is missing: apply the module options again")]
    MissingAsset(std::path::PathBuf),
    #[error("{operation} was rolled back: {failure}")]
    RolledBack { operation: String, failure: String },
    #[error("{0} was cancelled; run it again to resume")]
    Cancelled(String),
    #[error("cannot read the artifact index: {0}")]
    Index(String),
    #[error("download failed: {0}")]
    Download(#[from] rk_store::StoreError),
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Db(#[from] rk_db::DbError),
    #[error("'{0}' is not a computer address (e.g. 192.168.1.20)")]
    InvalidAddress(String),
    #[error("no backup '{0}'")]
    BackupNotFound(String),
    #[error("the backup '{0}' holds no player database of this realm")]
    BackupUnusable(String),
    #[error("{}: {reason}", path.display())]
    InvalidLocation {
        path: std::path::PathBuf,
        reason: String,
    },
    #[error("the install location can only change while no realm is installed")]
    LocationInUse,
    #[error("{} holds other files: choose an empty folder or an RealmKeeper folder", .0.display())]
    LocationNotEmpty(std::path::PathBuf),
    #[error(
        "the folder of realm {0} was renamed or moved: run `rk resume {0}` to update its paths"
    )]
    Moved(String),
    #[error("{} is synchronised by {client}: a database there would be corrupted; choose a folder outside it", path.display())]
    LocationSynced {
        path: std::path::PathBuf,
        client: String,
    },
    #[error("{} cannot hold the server files (NTFS folder links unsupported: {reason}); choose a folder on an internal NTFS drive", path.display())]
    LocationUnsupported {
        path: std::path::PathBuf,
        reason: String,
    },
    #[error("files of realm {instance} are missing (an antivirus may have removed them): {}", files.join(", "))]
    FilesMissing {
        instance: String,
        files: Vec<String>,
    },
    #[error("{0}")]
    NoSpace(#[source] rk_store::NoSpace),
    #[error("a realm '{0}' already exists")]
    InstanceExists(String),
    #[error("'{0}' is not a realm name (one line of at most 48 characters)")]
    InvalidName(String),
    #[error("not a realm exported by RealmKeeper: {0}")]
    ExportInvalid(String),
    #[error("the exported realm uses {0}, which this launcher does not offer")]
    ExportUnsupported(String),
    #[error(transparent)]
    Console(#[from] rk_core::console::ConsoleError),
    #[error(transparent)]
    Firewall(#[from] crate::firewall::FirewallError),
    #[error("{0}")]
    Invalid(String),
}

/// What the UI shows: `code` selects the translated message
/// (`error.<code>` in the locales), `params` fill its placeholders, `detail`
/// is the full technical chain, shown folded for bug reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiError {
    pub code: String,
    pub params: BTreeMap<String, String>,
    pub detail: String,
}

impl LauncherError {
    /// The full chain of causes, one per line.
    pub fn detail(&self) -> String {
        let mut text = self.to_string();
        let mut source = std::error::Error::source(self);
        while let Some(cause) = source {
            let line = cause.to_string();
            if !text.contains(&line) {
                text.push('\n');
                text.push_str(&line);
            }
            source = cause.source();
        }
        text
    }

    pub fn to_ui(&self) -> UiError {
        let (mut code, mut params) = self.code();
        // A taken port: name the program holding it, when it can be found.
        if code == "port.inUse" {
            let owner = params
                .iter()
                .find(|(k, _)| *k == "port")
                .and_then(|(_, port)| port.parse().ok())
                .and_then(rk_runtime::port_owner);
            if let Some(program) = owner {
                code = "port.inUseBy";
                params.push(("program", program));
            }
        }
        UiError {
            code: code.to_owned(),
            params: params.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
            detail: self.detail(),
        }
    }

    /// A stable code per actionable situation; everything else is `unknown`
    /// (the detail still tells what happened).
    pub fn code(&self) -> (&'static str, Vec<(&'static str, String)>) {
        use LauncherError as L;
        let none = Vec::new;
        // A full disk, wherever it happened (download, unpacking, dump).
        if let Some(full) = find_cause::<rk_store::NoSpace>(self) {
            return (
                "disk.full",
                vec![
                    ("needed", gigabytes(full.needed)),
                    ("available", gigabytes(full.available)),
                ],
            );
        }
        if find_cause::<std::io::Error>(self).is_some_and(rk_store::space::is_disk_full) {
            return ("disk.fullNow", none());
        }
        // ERROR_VIRUS_INFECTED / ERROR_VIRUS_DELETED: an antivirus refused
        // to run a program or removed it.
        if find_cause::<std::io::Error>(self)
            .and_then(std::io::Error::raw_os_error)
            .is_some_and(|code| code == 225 || code == 226)
        {
            return ("security.blocked", none());
        }
        match self {
            L::Catalog(_) => ("catalog.unreadable", none()),
            L::UnknownEmulator(_) | L::NoDriver(_) => ("catalog.invalid", none()),
            L::NoClient(version) => ("client.notRegistered", vec![("version", version.clone())]),
            L::MissingAsset(_) => ("assets.missing", none()),
            L::RolledBack { .. } => ("operation.rolledBack", none()),
            L::Cancelled(_) => ("operation.cancelled", none()),
            L::Index(_) => ("index.invalid", none()),
            L::Download(_) => ("network.download", none()),
            L::Plan(e) => plan_code(e),
            L::Engine(e) => engine_code(e),
            L::Runtime(e) => runtime_code(e),
            L::Client(e) => client_code(e),
            L::Db(e) => db_code(e),
            L::InvalidAddress(_) => ("lan.invalidAddress", none()),
            L::BackupNotFound(_) => ("backup.notFound", none()),
            L::BackupUnusable(_) => ("backup.unusable", none()),
            L::InvalidLocation { path, .. } => (
                "location.invalid",
                vec![("path", path.display().to_string())],
            ),
            L::LocationInUse => ("location.inUse", none()),
            L::Moved(_) => ("instance.moved", none()),
            L::Console(e) => console_code(e),
            L::LocationSynced { path, client } => (
                "location.synced",
                vec![
                    ("path", path.display().to_string()),
                    ("client", client.clone()),
                ],
            ),
            L::LocationUnsupported { path, .. } => (
                "location.unsupported",
                vec![("path", path.display().to_string())],
            ),
            L::FilesMissing { files, .. } => (
                "server.filesMissing",
                vec![
                    ("count", files.len().to_string()),
                    ("first", files.first().cloned().unwrap_or_default()),
                ],
            ),
            L::LocationNotEmpty(path) => (
                "location.notEmpty",
                vec![("path", path.display().to_string())],
            ),
            L::NoSpace(_) => ("disk.full", none()),
            L::InstanceExists(id) => ("realm.exists", vec![("id", id.clone())]),
            L::InvalidName(_) => ("realm.invalidName", none()),
            L::ExportInvalid(_) => ("realm.exportInvalid", none()),
            L::ExportUnsupported(what) => ("realm.exportUnsupported", vec![("what", what.clone())]),
            L::Firewall(e) => match e {
                crate::firewall::FirewallError::Declined => ("firewall.declined", none()),
                crate::firewall::FirewallError::Failed(_) => ("firewall.failed", none()),
                crate::firewall::FirewallError::Unsupported => ("firewall.unsupported", none()),
            },
            L::Invalid(_) => ("unknown", none()),
        }
    }
}

/// The first error of type `T` in the chain of causes of `error`.
fn find_cause<'a, T: std::error::Error + 'static>(
    error: &'a (dyn std::error::Error + 'static),
) -> Option<&'a T> {
    let mut current = Some(error);
    while let Some(e) = current {
        if let Some(found) = e.downcast_ref::<T>() {
            return Some(found);
        }
        current = e.source();
    }
    None
}

/// Bytes as gigabytes with one decimal (`8.6`), for messages.
fn gigabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1e9)
}

fn db_code(e: &rk_db::DbError) -> (&'static str, Vec<(&'static str, String)>) {
    match e {
        rk_db::DbError::PortInUse { port } => ("port.inUse", vec![("port", port.to_string())]),
        _ => ("database.failed", Vec::new()),
    }
}

fn plan_code(e: &PlanError) -> (&'static str, Vec<(&'static str, String)>) {
    match e {
        PlanError::InstanceBusy(_) => ("instance.busy", Vec::new()),
        PlanError::Unfinished(_) => ("instance.unfinished", Vec::new()),
        PlanError::InvalidId(_) => ("instance.invalidId", Vec::new()),
        PlanError::NoFreePort(_) => ("port.none", Vec::new()),
        PlanError::Engine(e) => engine_code(e),
        PlanError::Resolve(_) => ("modules.conflict", Vec::new()),
        _ => ("unknown", Vec::new()),
    }
}

fn engine_code(e: &EngineError) -> (&'static str, Vec<(&'static str, String)>) {
    match e {
        EngineError::Busy(_) => ("instance.busy", Vec::new()),
        EngineError::PlanChanged { .. } | EngineError::NeedsRepair { .. } => {
            ("instance.unfinished", Vec::new())
        }
        EngineError::StepFailed { step, source, .. } => {
            if step == "servers-stopped" {
                ("server.running", Vec::new())
            } else if source.to_string().contains("download") || step.starts_with("download") {
                ("network.download", Vec::new())
            } else {
                ("operation.failed", vec![("step", step.clone())])
            }
        }
        EngineError::RollbackFailed(_) => ("operation.rollbackFailed", Vec::new()),
        _ => ("unknown", Vec::new()),
    }
}

fn runtime_code(e: &RuntimeError) -> (&'static str, Vec<(&'static str, String)>) {
    match e {
        RuntimeError::PortInUse { port, .. } => ("port.inUse", vec![("port", port.to_string())]),
        RuntimeError::ExitedDuringStartup { process, .. }
        | RuntimeError::StartTimeout { process, .. } => {
            ("server.startFailed", vec![("process", process.clone())])
        }
        RuntimeError::MissingProgram(_) => ("instance.damaged", Vec::new()),
        RuntimeError::Db(e) => db_code(e),
        RuntimeError::Plan(e) => plan_code(e),
        RuntimeError::Module { .. } => ("modules.invalid", Vec::new()),
        RuntimeError::Driver { source, .. } => driver_code(source.as_ref()),
        _ => ("unknown", Vec::new()),
    }
}

fn console_code(e: &rk_core::console::ConsoleError) -> (&'static str, Vec<(&'static str, String)>) {
    use rk_core::console::ConsoleError as C;
    match e {
        C::Missing(p) => ("gm.missing", vec![("param", p.clone())]),
        C::Character(p) => ("gm.character", vec![("param", p.clone())]),
        C::Number { param, min, max } => (
            "gm.number",
            vec![
                ("param", param.clone()),
                ("min", min.to_string()),
                ("max", max.to_string()),
            ],
        ),
        C::Text { param, max } => (
            "gm.text",
            vec![("param", param.clone()), ("max", max.to_string())],
        ),
        C::Choice(p) => ("gm.choice", vec![("param", p.clone())]),
        C::UnknownPlaceholder(_) => ("catalog.invalid", Vec::new()),
    }
}

fn driver_code(
    e: &(dyn std::error::Error + 'static),
) -> (&'static str, Vec<(&'static str, String)>) {
    use rk_driver_azerothcore::AccountError as A;
    match e.downcast_ref::<A>() {
        Some(A::InvalidName) => ("account.invalidName", Vec::new()),
        Some(A::InvalidPassword) => ("account.invalidPassword", Vec::new()),
        Some(A::Exists(name)) => ("account.exists", vec![("name", name.clone())]),
        Some(A::UnknownAccount) => ("account.unknown", Vec::new()),
        Some(A::InvalidGmLevel) => ("account.invalidGmLevel", Vec::new()),
        _ => ("unknown", Vec::new()),
    }
}

fn client_code(e: &ClientError) -> (&'static str, Vec<(&'static str, String)>) {
    match e {
        ClientError::NotFound(_) | ClientError::MissingExecutable(_) => {
            ("client.notFound", Vec::new())
        }
        ClientError::WrongBuild {
            found,
            expected,
            version,
            ..
        } => (
            "client.wrongBuild",
            vec![
                ("found", found.to_string()),
                ("expected", expected.to_string()),
                ("version", version.clone()),
            ],
        ),
        ClientError::UnreadableVersion(_) => ("client.notFound", Vec::new()),
        ClientError::NoLocale(_) | ClientError::UnknownLocale { .. } => {
            ("client.locale", Vec::new())
        }
        ClientError::AmbiguousLocale(found) => {
            ("client.ambiguousLocale", vec![("found", found.clone())])
        }
        ClientError::NotWritable(_) => ("client.notWritable", Vec::new()),
        ClientError::InUse(_) => ("client.inUse", Vec::new()),
        ClientError::Busy(_) => ("client.busy", Vec::new()),
        ClientError::Conflict { .. } => ("client.conflict", Vec::new()),
        _ => ("unknown", Vec::new()),
    }
}

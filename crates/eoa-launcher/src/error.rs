use std::collections::BTreeMap;

use eoa_client::ClientError;
use eoa_engine::install::PlanError;
use eoa_engine::EngineError;
use eoa_runtime::RuntimeError;
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
    Download(#[from] eoa_store::StoreError),
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Db(#[from] eoa_db::DbError),
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
        let (code, params) = self.code();
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
            L::Db(_) => ("database.failed", none()),
            L::Invalid(_) => ("unknown", none()),
        }
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
        RuntimeError::Plan(e) => plan_code(e),
        RuntimeError::Module { .. } => ("modules.invalid", Vec::new()),
        RuntimeError::Driver { source, .. } => driver_code(source.as_ref()),
        _ => ("unknown", Vec::new()),
    }
}

fn driver_code(
    e: &(dyn std::error::Error + 'static),
) -> (&'static str, Vec<(&'static str, String)>) {
    use eoa_driver_azerothcore::AccountError as A;
    match e.downcast_ref::<A>() {
        Some(A::InvalidName) => ("account.invalidName", Vec::new()),
        Some(A::InvalidPassword) => ("account.invalidPassword", Vec::new()),
        Some(A::Exists(name)) => ("account.exists", vec![("name", name.clone())]),
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

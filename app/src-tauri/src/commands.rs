//! Commands exposed to the UI. Keep in sync with `app/src/lib/api.ts`.
//!
//! Every command forwards to `eoa-launcher` (ADR 0014); this file only maps
//! arguments, errors ([`UiError`]) and events:
//! - `operation`: [`OperationPayload`] for install / update / module changes;
//! - `session`: [`SessionPayload`], session events batched every 100 ms
//!   (a world start prints thousands of lines).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eoa_core::manifest::ModuleCategory;
use eoa_core::LocalizedText;
use eoa_engine::install::{InstanceRecord, Network};
use eoa_engine::CancelToken;
use eoa_launcher::backups::Backup;
use eoa_launcher::progress::{EventObserver, OperationEvent};
use eoa_launcher::session::{Game, Session, SessionEvent};
use eoa_launcher::{
    InstallRequest, InstanceSummary, Launcher, LauncherError, ModuleOptions, UiError, UpdateOutcome,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

type Result<T> = std::result::Result<T, UiError>;

fn ui(error: LauncherError) -> UiError {
    error.to_ui()
}

fn app_error(code: &str, detail: impl ToString) -> UiError {
    UiError {
        code: code.into(),
        params: BTreeMap::new(),
        detail: detail.to_string(),
    }
}

/// One change at a time: two operations would fight over the same files
/// (the engine and instance locks refuse it anyway, less clearly).
struct ActiveOperation {
    instance: String,
    cancel: CancelToken,
}

pub struct AppState {
    launcher: std::result::Result<Arc<Launcher>, UiError>,
    operation: Mutex<Option<ActiveOperation>>,
    sessions: Mutex<BTreeMap<String, Session>>,
}

impl AppState {
    pub fn open(app: &AppHandle) -> Self {
        let launcher = match catalog_dir(app) {
            None => Err(app_error("catalog.missing", "bundled catalog not found")),
            Some(dir) => Launcher::open(&dir, &eoa_launcher::default_root())
                .map(Arc::new)
                .map_err(ui),
        };
        Self {
            launcher,
            operation: Mutex::new(None),
            sessions: Mutex::new(BTreeMap::new()),
        }
    }

    fn launcher(&self) -> Result<Arc<Launcher>> {
        self.launcher.clone()
    }

    fn running(&self, id: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, s| !s.is_finished());
        sessions.contains_key(id)
    }

    /// Ids of the instances with a session.
    pub fn running_ids(&self) -> Vec<String> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, s| !s.is_finished());
        sessions.keys().cloned().collect()
    }

    /// Stops every session and cancels the running operation (app closing);
    /// returns once the servers are down.
    pub async fn shutdown(&self) {
        if let Some(active) = self
            .operation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            active.cancel.cancel();
        }
        let sessions: Vec<Session> =
            std::mem::take(&mut *self.sessions.lock().unwrap_or_else(|e| e.into_inner()))
                .into_values()
                .collect();
        for session in &sessions {
            session.stop();
        }
        for session in sessions {
            session.wait().await;
        }
    }
}

/// Catalog location: the bundled resource. Debug builds fall back to the
/// repository copy (the path is compiled into debug binaries only).
fn catalog_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(resources) = app.path().resource_dir() {
        let bundled = resources.join("catalog");
        if bundled.join("emulators").is_dir() {
            return Some(bundled);
        }
    }
    #[cfg(debug_assertions)]
    {
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog"))
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

// ------------------------------------------------------------------ catalog

#[derive(Serialize)]
pub struct ModuleSummary {
    id: String,
    name: LocalizedText,
    description: LocalizedText,
    category: ModuleCategory,
    homepage: String,
    mandatory: bool,
}

/// How the side bar presents the client an emulator serves.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSummary {
    id: String,
    name: LocalizedText,
    short_name: LocalizedText,
    version: String,
    build: u32,
    emblem: String,
    accent: String,
}

#[derive(Serialize)]
pub struct EmulatorOverview {
    id: String,
    name: String,
    description: LocalizedText,
    client: Option<ClientSummary>,
    /// Server processes in start order (the database is implicit and first).
    processes: Vec<String>,
    /// The process whose stdin is the server console (stopped by closing
    /// stdin, ADR 0011), if any.
    console: Option<String>,
    modules: Vec<ModuleSummary>,
}

#[tauri::command]
pub fn catalog_overview(state: State<'_, AppState>) -> Result<Vec<EmulatorOverview>> {
    let launcher = state.launcher()?;
    let catalog = launcher.catalog();
    Ok(catalog
        .emulators
        .values()
        .map(|entry| {
            let mandatory =
                catalog.mandatory_modules(&entry.manifest.id, &entry.manifest.default_flavor);
            let mut modules: Vec<ModuleSummary> = entry
                .modules
                .values()
                .filter(|m| {
                    m.compatible_flavors
                        .contains(&entry.manifest.default_flavor)
                })
                .map(|m| ModuleSummary {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    description: m.description.clone(),
                    category: m.category,
                    homepage: m.homepage.clone(),
                    mandatory: mandatory.contains(&m.id),
                })
                .collect();
            // Mandatory modules first, then alphabetical by id.
            modules.sort_by(|a, b| b.mandatory.cmp(&a.mandatory).then_with(|| a.id.cmp(&b.id)));
            let client = catalog
                .clients
                .get(&entry.manifest.client)
                .map(|c| ClientSummary {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    short_name: c.branding.short_name.clone(),
                    version: c.version.clone(),
                    build: c.build,
                    emblem: c.branding.emblem.clone(),
                    accent: c.branding.accent.clone(),
                });
            EmulatorOverview {
                id: entry.manifest.id.clone(),
                name: entry.manifest.name.clone(),
                description: entry.manifest.description.clone(),
                client,
                processes: entry
                    .manifest
                    .processes
                    .iter()
                    .map(|p| p.id.clone())
                    .collect(),
                console: entry
                    .manifest
                    .processes
                    .iter()
                    .find(|p| matches!(p.stop, eoa_core::manifest::StopMethod::CloseStdin { .. }))
                    .map(|p| p.id.clone()),
                modules,
            }
        })
        .collect())
}

// ---------------------------------------------------------------- overview

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceView {
    #[serde(flatten)]
    summary: InstanceSummary,
    running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientView {
    id: String,
    path: PathBuf,
    locale: String,
    managed: Vec<String>,
    /// Why the registered folder no longer works (moved, other build...).
    problem: Option<UiError>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherOverview {
    root: PathBuf,
    instances: Vec<InstanceView>,
    clients: Vec<ClientView>,
    /// Instance whose operation is running, if any.
    busy: Option<String>,
}

fn client_view(launcher: &Launcher, id: &str) -> Result<Option<ClientView>> {
    let Some(state) = launcher.client(id).map_err(ui)? else {
        return Ok(None);
    };
    let problem = launcher
        .catalog()
        .clients
        .get(id)
        .and_then(|manifest| eoa_client::detect(manifest, &state.path).err())
        .map(|e| LauncherError::from(e).to_ui());
    Ok(Some(ClientView {
        id: state.client,
        path: state.path,
        locale: state.locale,
        managed: state.managed.into_iter().map(|f| f.path).collect(),
        problem,
    }))
}

#[tauri::command]
pub fn launcher_overview(state: State<'_, AppState>) -> Result<LauncherOverview> {
    let launcher = state.launcher()?;
    let instances = launcher
        .instances()
        .map_err(ui)?
        .into_iter()
        .map(|summary| InstanceView {
            running: state.running(&summary.record.id),
            summary,
        })
        .collect();
    let mut clients = Vec::new();
    for id in launcher.catalog().clients.keys() {
        if let Some(view) = client_view(&launcher, id)? {
            clients.push(view);
        }
    }
    let busy = state
        .operation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|a| a.instance.clone());
    Ok(LauncherOverview {
        root: launcher.layout().root.clone(),
        instances,
        clients,
        busy,
    })
}

#[tauri::command]
pub fn register_client(
    state: State<'_, AppState>,
    client: String,
    path: PathBuf,
    locale: Option<String>,
) -> Result<ClientView> {
    let launcher = state.launcher()?;
    launcher
        .register_client(&client, &path, locale.as_deref())
        .map_err(ui)?;
    client_view(&launcher, &client)?.ok_or_else(|| app_error("unknown", "client not saved"))
}

// --------------------------------------------------------------- operations

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationPayload {
    instance: String,
    event: OperationEvent,
}

/// Runs one change with progress events; refuses a second one.
async fn operation<T, F, Fut>(app: AppHandle, state: &AppState, instance: &str, run: F) -> Result<T>
where
    F: FnOnce(Arc<Launcher>, EventObserver, CancelToken) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, LauncherError>>,
{
    let launcher = state.launcher()?;
    if state.running(instance) {
        return Err(app_error("server.running", instance));
    }
    let cancel = CancelToken::new();
    {
        let mut active = state.operation.lock().unwrap_or_else(|e| e.into_inner());
        if active.is_some() {
            return Err(app_error("operation.busy", instance));
        }
        *active = Some(ActiveOperation {
            instance: instance.to_owned(),
            cancel: cancel.clone(),
        });
    }
    let id = instance.to_owned();
    let observer = EventObserver::new(Arc::new(move |event| {
        let _ = app.emit(
            "operation",
            OperationPayload {
                instance: id.clone(),
                event,
            },
        );
    }));
    // Released even if the future is dropped or panics.
    let _guard = OperationGuard(state);
    run(launcher, observer, cancel).await.map_err(ui)
}

struct OperationGuard<'a>(&'a AppState);

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        *self.0.operation.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallArgs {
    id: String,
    emulator: String,
    modules: Vec<String>,
    lan: bool,
}

#[tauri::command]
pub async fn install_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    request: InstallArgs,
) -> Result<InstanceRecord> {
    let id = request.id.clone();
    let request = InstallRequest {
        id: request.id,
        emulator: request.emulator,
        flavor: None,
        modules: request.modules,
        network: if request.lan {
            Network::Lan
        } else {
            Network::Solo
        },
        server: None,
        db_snapshot: None,
        index: None,
    };
    operation(app, &state, &id, |launcher, observer, cancel| async move {
        launcher.install(&request, &observer, &cancel).await
    })
    .await
}

#[tauri::command]
pub async fn update_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<UpdateOutcome> {
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move {
            launcher.update(&id, None, None, &observer, &cancel).await
        },
    )
    .await
}

#[tauri::command]
pub async fn repair_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<InstanceRecord> {
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move { launcher.repair(&id, &observer, &cancel).await },
    )
    .await
}

#[tauri::command]
pub async fn resume_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<InstanceRecord> {
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move { launcher.resume(&id, &observer, &cancel).await },
    )
    .await
}

#[tauri::command]
pub fn rebuilds_for(
    state: State<'_, AppState>,
    id: String,
    disable: Vec<String>,
) -> Result<Vec<String>> {
    state.launcher()?.rebuilds_for(&id, &disable).map_err(ui)
}

#[tauri::command]
pub async fn set_modules(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enable: Vec<String>,
    disable: Vec<String>,
) -> Result<InstanceRecord> {
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move {
            launcher
                .set_modules(&id, &enable, &disable, &observer, &cancel)
                .await
        },
    )
    .await
}

#[tauri::command]
pub fn instance_options(state: State<'_, AppState>, id: String) -> Result<Vec<ModuleOptions>> {
    let launcher = state.launcher()?;
    let record = launcher.instance(&id).map_err(ui)?.record;
    Ok(launcher.options(&record))
}

#[tauri::command]
pub async fn set_options(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    module: String,
    values: Vec<(String, String)>,
) -> Result<InstanceRecord> {
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move {
            launcher
                .set_options(&id, &module, &values, &observer, &cancel)
                .await
        },
    )
    .await
}

/// Cancels the running operation: it stops at the next step boundary and
/// rolls back (or resumes later, for an install).
#[tauri::command]
pub fn cancel_operation(state: State<'_, AppState>) {
    if let Some(active) = state
        .operation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        active.cancel.cancel();
    }
}

#[tauri::command]
pub async fn create_account(
    state: State<'_, AppState>,
    id: String,
    username: String,
    password: String,
    gm_level: u8,
) -> Result<u32> {
    let launcher = state.launcher()?;
    let account = eoa_runtime::NewAccount {
        username,
        password,
        gm_level,
    };
    launcher.create_account(&id, &account).await.map_err(ui)
}

// ---------------------------------------------------------------------- lan

/// Points the game at a realm hosted on another computer and launches it;
/// the client stays locked until the game exits.
#[tauri::command]
pub fn join_lan(state: State<'_, AppState>, emulator: String, address: String) -> Result<()> {
    let launcher = state.launcher()?;
    let client = launcher
        .prepare_client_for_lan(&emulator, &address)
        .map_err(ui)?;
    let mut child = eoa_client::launch(&client.manifest, &client.state)
        .map_err(|e| ui(LauncherError::Client(e)))?;
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        drop(client);
    });
    Ok(())
}

// ------------------------------------------------------------------ backups

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>, id: String) -> Result<Vec<Backup>> {
    state.launcher()?.backups(&id).map_err(ui)
}

#[tauri::command]
pub async fn backup_now(state: State<'_, AppState>, id: String) -> Result<Backup> {
    let launcher = state.launcher()?;
    if state.operation.lock().expect("operation lock").is_some() {
        return Err(app_error("operation.busy", &id));
    }
    launcher.backup_now(&id).await.map_err(ui)
}

#[tauri::command]
pub fn delete_backup(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    state.launcher()?.delete_backup(&id, &name).map_err(ui)
}

#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<InstanceRecord> {
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move {
            launcher.restore(&id, &name, &observer, &cancel).await
        },
    )
    .await
}

/// Shows the backups folder in the file explorer (created if needed).
#[tauri::command]
pub fn open_backups_folder(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state.launcher()?.backups_dir(&id);
    std::fs::create_dir_all(&dir).map_err(|e| app_error("backup.folder", e))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| app_error("backup.folder", e))
}

// ----------------------------------------------------------------- sessions

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionPayload {
    instance: String,
    events: Vec<SessionEvent>,
}

/// Forwards session events in batches until the session ends.
fn forward_session_events(app: AppHandle, instance: String) -> eoa_launcher::session::EventSink {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();
    tauri::async_runtime::spawn(async move {
        while let Some(first) = rx.recv().await {
            let mut events = vec![first];
            tokio::time::sleep(Duration::from_millis(100)).await;
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            let _ = app.emit(
                "session",
                SessionPayload {
                    instance: instance.clone(),
                    events,
                },
            );
        }
    });
    Arc::new(move |event| {
        let _ = tx.send(event);
    })
}

/// Starts an instance; with `play`, prepares the client first and launches
/// the game once the servers are ready.
#[tauri::command]
pub async fn start_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    play: bool,
    keep_running: bool,
) -> Result<()> {
    let launcher = state.launcher()?;
    if state.running(&id) {
        return Err(app_error("server.running", &id));
    }
    if let Some(active) = state
        .operation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        if active.instance == id {
            return Err(app_error("operation.busy", &id));
        }
    }
    let server = eoa_runtime::Server::open(launcher.catalog(), launcher.layout(), &id)
        .map_err(|e| ui(e.into()))?;
    let game = if play {
        Some(Game {
            client: launcher.prepare_client(server.record()).map_err(ui)?,
            keep_running,
        })
    } else {
        None
    };
    let events = forward_session_events(app, id.clone());
    let session = Session::start(server, game, events).map_err(ui)?;
    state
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, session);
    Ok(())
}

#[tauri::command]
pub fn stop_instance(state: State<'_, AppState>, id: String) {
    if let Some(session) = state
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
    {
        session.stop();
    }
}

#[tauri::command]
pub fn console_command(state: State<'_, AppState>, id: String, line: String) -> Result<()> {
    match state
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
    {
        Some(session) => {
            session.console(&line);
            Ok(())
        }
        None => Err(app_error("server.notRunning", &id)),
    }
}

// ------------------------------------------------------------ crash reports

/// Reports of earlier crashes, already redacted; the UI offers to open a
/// pre-filled issue (the player submits it, or not).
#[tauri::command]
pub fn crash_reports() -> Vec<eoa_launcher::crash::CrashReport> {
    eoa_launcher::crash::reports(&eoa_launcher::default_root())
}

#[tauri::command]
pub fn dismiss_crash_report(id: String) -> Result<()> {
    eoa_launcher::crash::dismiss(&eoa_launcher::default_root(), &id)
        .map_err(|e| app_error("unknown", e))
}

// ---------------------------------------------------------- launcher update

/// Whether this build carries a signed update feed (`plugins.updater`).
pub fn has_update_feed(app: &AppHandle) -> bool {
    app.config().plugins.0.contains_key("updater")
}

#[derive(Serialize)]
pub struct AppUpdate {
    version: String,
    notes: Option<String>,
}

async fn pending_app_update(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>> {
    use tauri_plugin_updater::UpdaterExt;
    if !has_update_feed(app) {
        return Ok(None);
    }
    let updater = app
        .updater()
        .map_err(|e| app_error("appUpdate.failed", e))?;
    updater
        .check()
        .await
        .map_err(|e| app_error("network.download", e))
}

/// A newer launcher, if the signed feed has one.
#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> Result<Option<AppUpdate>> {
    Ok(pending_app_update(&app).await?.map(|u| AppUpdate {
        version: u.version.clone(),
        notes: u.body.clone(),
    }))
}

/// Downloads the update, checks its signature, runs the installer and
/// restarts. Refused while servers run (the installer closes the launcher).
#[tauri::command]
pub async fn install_app_update(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    if !state.running_ids().is_empty() {
        return Err(app_error("server.running", "servers are running"));
    }
    if state
        .operation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
    {
        return Err(app_error("operation.busy", "an operation is running"));
    }
    let update = pending_app_update(&app)
        .await?
        .ok_or_else(|| app_error("appUpdate.none", "no update available"))?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| app_error("appUpdate.failed", e))?;
    app.restart()
}

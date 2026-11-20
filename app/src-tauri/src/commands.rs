//! Commands exposed to the UI. Keep in sync with `app/src/lib/api.ts`.
//!
//! Every command forwards to `rk-launcher` (ADR 0014); this file only maps
//! arguments, errors ([`UiError`]) and events:
//! - `operation`: [`OperationPayload`] for install / update / module changes;
//! - `session`: [`SessionPayload`], session events batched every 100 ms
//!   (a world start prints thousands of lines).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rk_engine::install::{InstanceRecord, Network};
use rk_engine::CancelToken;
use rk_launcher::backups::Backup;
use rk_launcher::firewall::FirewallStatus;
use rk_launcher::overview::EmulatorOverview;
use rk_launcher::preferences::{self, Preferences};
use rk_launcher::progress::{EventObserver, OperationEvent};
use rk_launcher::realms::ExportSummary;
use rk_launcher::session::{Session, SessionControl, SessionEvent};
use rk_launcher::{
    InstallRequest, InstanceSummary, Launcher, LauncherError, ModuleOptions, ServerUpdate,
    SettingsView, SpaceEstimate, UiError, UninstallReport, UpdateOutcome,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

type Result<T> = std::result::Result<T, UiError>;
/// A realm's database turn (`Launcher::database_turn`).
type Turn = tokio::sync::OwnedMutexGuard<()>;

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
    /// Reopened when the player picks another install location.
    launcher: Mutex<std::result::Result<Arc<Launcher>, UiError>>,
    catalog: Option<PathBuf>,
    operation: Mutex<Option<ActiveOperation>>,
    sessions: Mutex<BTreeMap<String, Session>>,
    /// Work started after a session ended (the daily backup), by instance:
    /// the next start or change of that instance waits for it.
    background: Mutex<BTreeMap<String, tauri::async_runtime::JoinHandle<()>>>,
    /// The launcher is quitting: no new background work.
    closing: AtomicBool,
    /// The window was hidden while the game runs (shown again after).
    hidden_for_game: AtomicBool,
    /// Texts Windows shows for the launcher, in the player's language.
    os_texts: Mutex<OsTexts>,
}

/// What Windows shows while the launcher keeps it awake or delays a
/// shutdown (ADR 0020).
struct OsTexts {
    shutdown: String,
    awake: String,
    /// Notification when a realm is ready while the window is hidden.
    ready: String,
}

impl Default for OsTexts {
    fn default() -> Self {
        Self {
            shutdown: "RealmKeeper is stopping your servers cleanly to keep your characters safe."
                .into(),
            awake: "RealmKeeper is installing or updating a realm.".into(),
            ready: "Your realm is ready: you can log in.".into(),
        }
    }
}

impl AppState {
    pub fn open(app: &AppHandle) -> Self {
        let catalog = catalog_dir(app);
        let launcher = Self::open_launcher(catalog.as_deref());
        Self {
            launcher: Mutex::new(launcher),
            catalog,
            operation: Mutex::new(None),
            sessions: Mutex::new(BTreeMap::new()),
            background: Mutex::new(BTreeMap::new()),
            closing: AtomicBool::new(false),
            hidden_for_game: AtomicBool::new(false),
            os_texts: Mutex::new(OsTexts::default()),
        }
    }

    /// Waits for the background work of `id` (a daily backup holds the
    /// instance lock for a few seconds), then for the realm's database turn
    /// (`Launcher::database_turn`). Keep the turn until the session or
    /// operation is registered, and check again under it what was checked
    /// before: work that ran meanwhile may have changed it.
    async fn settle(&self, launcher: &Launcher, id: &str) -> Turn {
        let pending = self
            .background
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(handle) = pending {
            let _ = handle.await;
        }
        launcher.database_turn(id).await
    }

    fn open_launcher(catalog: Option<&std::path::Path>) -> Result<Arc<Launcher>> {
        match catalog {
            None => Err(app_error("catalog.missing", "bundled catalog not found")),
            Some(dir) => Launcher::open(dir, &rk_launcher::data_root().root)
                .map(Arc::new)
                .map_err(ui),
        }
    }

    fn launcher(&self) -> Result<Arc<Launcher>> {
        self.launcher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn running(&self, id: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, s| !s.is_finished());
        sessions.contains_key(id)
    }

    /// Controls of the instance's running session, if any.
    fn session_control(&self, id: &str) -> Option<SessionControl> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, s| !s.is_finished());
        sessions.get(id).map(Session::control)
    }

    /// Where database work on `id` goes (`Launcher::on_database`): its
    /// session, or the database itself unless a change is in progress.
    fn database_route(&self, id: &str) -> Result<Option<SessionControl>> {
        if let Some(control) = self.session_control(id) {
            return Ok(Some(control));
        }
        if self.operation.lock().expect("operation lock").is_some() {
            return Err(app_error("operation.busy", id));
        }
        Ok(None)
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
        self.closing.store(true, Ordering::SeqCst);
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
        // A backup in progress finishes (its database stops cleanly).
        let background: Vec<_> =
            std::mem::take(&mut *self.background.lock().unwrap_or_else(|e| e.into_inner()))
                .into_values()
                .collect();
        for handle in background {
            let _ = handle.await;
        }
    }
}

/// Asks Windows to wait at shutdown or log-off while servers run or an
/// operation works (ADR 0020); `ending` is a session about to end.
pub fn update_shutdown_block(app: &AppHandle, ending: Option<&str>) {
    let state = app.state::<AppState>();
    let busy = state
        .running_ids()
        .iter()
        .any(|id| Some(id.as_str()) != ending)
        || state
            .operation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some();
    let reason = busy.then(|| {
        state
            .os_texts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutdown
            .clone()
    });
    if let Some(window) = app.get_webview_window("main") {
        // Windows only accepts the block reason from the thread that made
        // the window: callers run on async workers, so hop to the main one.
        let target = window.clone();
        // Fails only once the event loop has ended: nothing left to block.
        let _ = window.run_on_main_thread(move || {
            crate::os::block_shutdown(&target, reason.as_deref());
        });
    }
}

/// Asks every running session to stop cleanly (tray menu).
pub fn stop_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    for session in state
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
    {
        session.stop();
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

#[tauri::command]
pub fn catalog_overview(state: State<'_, AppState>) -> Result<Vec<EmulatorOverview>> {
    Ok(state.launcher()?.catalog_overview())
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
        .and_then(|manifest| rk_client::detect(manifest, &state.path).err())
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

/// A game folder found by [`find_clients`].
#[derive(Serialize)]
pub struct FoundClient {
    path: PathBuf,
    locales: Vec<String>,
}

/// Looks for the game in the usual places (a few seconds at most).
#[tauri::command]
pub async fn find_clients(state: State<'_, AppState>, client: String) -> Result<Vec<FoundClient>> {
    let launcher = state.launcher()?;
    let manifest = launcher
        .catalog()
        .clients
        .get(&client)
        .cloned()
        .ok_or_else(|| app_error("catalog.invalid", &client))?;
    let found = tauri::async_runtime::spawn_blocking(move || rk_client::find(&manifest))
        .await
        .map_err(|e| app_error("unknown", e))?;
    Ok(found
        .into_iter()
        .map(|d| FoundClient {
            path: d.root,
            locales: d.locales,
        })
        .collect())
}

/// What a first install of `emulator` downloads and needs on the disk.
#[tauri::command]
pub fn install_space(state: State<'_, AppState>, emulator: String) -> Result<SpaceEstimate> {
    state.launcher()?.install_space(&emulator).map_err(ui)
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
    let settled = state.settle(&launcher, instance).await;
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
    drop(settled);
    // Awake and shutdown-safe while it works (ADR 0020).
    let awake_text = state
        .os_texts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .awake
        .clone();
    let _awake = crate::os::KeepAwake::new(&awake_text);
    update_shutdown_block(&app, None);
    let handle = app.clone();
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
    let _guard = OperationGuard(state, handle);
    run(launcher, observer, cancel).await.map_err(ui)
}

struct OperationGuard<'a>(&'a AppState, AppHandle);

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        *self.0.operation.lock().unwrap_or_else(|e| e.into_inner()) = None;
        update_shutdown_block(&self.1, None);
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

/// What the signed index offers the realm (only the index is downloaded).
#[tauri::command]
pub async fn server_update(state: State<'_, AppState>, id: String) -> Result<ServerUpdate> {
    let launcher = state.launcher()?;
    launcher.server_update(&id).await.map_err(ui)
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

/// Removes an installed realm; with `keep_backup`, its player data is
/// backed up first (restorable after installing it again).
#[tauri::command]
pub async fn uninstall_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    keep_backup: bool,
) -> Result<UninstallReport> {
    let instance = id.clone();
    operation(app, &state, &instance, |launcher, _, _| async move {
        launcher.uninstall(&id, keep_backup).await
    })
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
pub fn realm_settings(state: State<'_, AppState>, id: String) -> Result<Vec<SettingsView>> {
    let launcher = state.launcher()?;
    let record = launcher.instance(&id).map_err(ui)?.record;
    Ok(launcher.realm_settings(&record))
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
    let account = rk_runtime::NewAccount {
        username,
        password,
        gm_level,
    };
    let realm = id.clone();
    launcher
        .on_database(
            &id,
            || state.database_route(&id),
            |launcher| async move { launcher.create_account(&realm, &account).await },
        )
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    id: u32,
    username: String,
    gm_level: u8,
}

/// Player accounts and their GM level.
#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>, id: String) -> Result<Vec<AccountView>> {
    let launcher = state.launcher()?;
    let realm = id.clone();
    Ok(launcher
        .on_database(
            &id,
            || state.database_route(&id),
            |launcher| async move { launcher.accounts(&realm).await },
        )
        .await?
        .into_iter()
        .map(|a| AccountView {
            id: a.id,
            username: a.username,
            gm_level: a.gm_level,
        })
        .collect())
}

#[tauri::command]
pub async fn set_gm_level(
    state: State<'_, AppState>,
    id: String,
    username: String,
    gm_level: u8,
) -> Result<()> {
    let launcher = state.launcher()?;
    let realm = id.clone();
    launcher
        .on_database(
            &id,
            || state.database_route(&id),
            |launcher| async move { launcher.set_gm_level(&realm, &username, gm_level).await },
        )
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineCharacterView {
    name: String,
    level: u8,
    race: String,
    class: String,
    bot: bool,
}

/// Most characters listed at once (players come first).
const ONLINE_LIMIT: u32 = 300;

/// Characters in the world; `null` while the servers are stopped.
#[tauri::command]
pub async fn online_characters(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<Vec<OnlineCharacterView>>> {
    if !state.running(&id) {
        return Ok(None);
    }
    let launcher = state.launcher()?;
    Ok(launcher
        .online_characters(&id, ONLINE_LIMIT)
        .await
        .map_err(ui)?
        .map(|list| {
            list.into_iter()
                .map(|c| OnlineCharacterView {
                    name: c.name,
                    level: c.level,
                    race: c.race,
                    class: c.class,
                    bot: c.bot,
                })
                .collect()
        }))
}

/// Teleport places of the GM panel (empty while the servers are stopped).
#[tauri::command]
pub async fn teleport_locations(state: State<'_, AppState>, id: String) -> Result<Vec<String>> {
    if !state.running(&id) {
        return Ok(Vec::new());
    }
    state.launcher()?.teleport_locations(&id).await.map_err(ui)
}

/// Runs a GM panel action: its command, values checked, is typed in the
/// server console (the answer shows in the console log). Returns the line.
#[tauri::command]
pub fn run_console_action(
    state: State<'_, AppState>,
    id: String,
    action: String,
    values: BTreeMap<String, String>,
) -> Result<String> {
    let line = state
        .launcher()?
        .console_action_line(&id, &action, &values)
        .map_err(ui)?;
    match state
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
    {
        Some(session) => {
            session.console(&line);
            Ok(line)
        }
        None => Err(app_error("server.notRunning", &id)),
    }
}

// ---------------------------------------------------------------------- lan

/// Points the game at a realm hosted on another computer and launches it;
/// the client stays locked until the game exits.
#[tauri::command]
pub fn join_lan(
    app: AppHandle,
    state: State<'_, AppState>,
    emulator: String,
    address: String,
) -> Result<()> {
    let launcher = state.launcher()?;
    let client = launcher
        .prepare_client_for_lan(&emulator, &address)
        .map_err(ui)?;
    let mut child = rk_client::launch(&client.manifest, &client.state)
        .map_err(|e| ui(LauncherError::Client(e)))?;
    game_started(&app);
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        drop(client);
        game_ended(&app);
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
    // Not under the turn while it runs: the realm's session keeps its
    // database up (a stop waits for the dump).
    if let Some(control) = state.session_control(&id) {
        return backup_live(launcher, control, id).await;
    }
    // A stopped realm: the backup starts its database, so it holds the
    // turn throughout. Started meanwhile: backed up through its session.
    let turn = state.settle(&launcher, &id).await;
    if let Some(control) = state.session_control(&id) {
        drop(turn);
        return backup_live(launcher, control, id).await;
    }
    if state.operation.lock().expect("operation lock").is_some() {
        return Err(app_error("operation.busy", &id));
    }
    let made = launcher.backup_now(&id).await.map_err(ui);
    drop(turn);
    made
}

/// A backup of a running realm: its session saves the players, then runs
/// the dump while it keeps the database up (ADR 0022).
async fn backup_live(
    launcher: Arc<Launcher>,
    control: SessionControl,
    id: String,
) -> Result<Backup> {
    let realm = id.clone();
    match control
        .while_running(async move { launcher.backup_live(&realm).await })
        .await
    {
        Some(made) => made.map_err(ui),
        None => Err(app_error("backup.notReady", &id)),
    }
}

#[tauri::command]
pub fn delete_backup(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    if state.operation.lock().expect("operation lock").is_some() {
        return Err(app_error("operation.busy", &id));
    }
    state.launcher()?.delete_backup(&id, &name).map_err(ui)
}

// --------------------------------------------------- firewall (ADR 0022)

#[tauri::command]
pub async fn firewall_status(state: State<'_, AppState>, id: String) -> Result<FirewallStatus> {
    let launcher = state.launcher()?;
    launcher.firewall_status(&id).await.map_err(ui)
}

/// Makes the realm's firewall rules; Windows shows its administrator prompt.
#[tauri::command]
pub async fn allow_firewall(state: State<'_, AppState>, id: String) -> Result<FirewallStatus> {
    let launcher = state.launcher()?;
    launcher.allow_firewall(&id).await.map_err(ui)
}

#[tauri::command]
pub async fn remove_firewall(state: State<'_, AppState>, id: String) -> Result<FirewallStatus> {
    let launcher = state.launcher()?;
    launcher.remove_firewall(&id).await.map_err(ui)
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

// ------------------------------------------------------- realms (ADR 0020)

/// Copies the stopped realm `source` to a new realm `id`. Progress is
/// reported under the new id.
#[tauri::command]
pub async fn duplicate_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    source: String,
    id: String,
    name: Option<String>,
) -> Result<InstanceRecord> {
    if state.running(&source) {
        return Err(app_error("server.running", &source));
    }
    // Only waited for: the operation below settles again.
    let launcher = state.launcher()?;
    drop(state.settle(&launcher, &source).await);
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move {
            launcher
                .duplicate(&source, &id, name.as_deref(), &observer, &cancel)
                .await
        },
    )
    .await
}

/// Writes the stopped realm `id` to the file `path` (`.rkrealm`).
#[tauri::command]
pub async fn export_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: PathBuf,
) -> Result<ExportSummary> {
    let instance = id.clone();
    operation(app, &state, &instance, |launcher, _, _| async move {
        launcher.export(&id, &path).await
    })
    .await
}

/// What an exported realm file holds (to confirm before importing it).
#[tauri::command]
pub fn read_export(state: State<'_, AppState>, path: PathBuf) -> Result<ExportSummary> {
    state.launcher()?.read_export(&path).map_err(ui)
}

/// Installs a new realm `id` from an exported file.
#[tauri::command]
pub async fn import_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
    id: String,
    name: Option<String>,
) -> Result<InstanceRecord> {
    let instance = id.clone();
    operation(
        app,
        &state,
        &instance,
        |launcher, observer, cancel| async move {
            launcher
                .import(&path, &id, name.as_deref(), None, &observer, &cancel)
                .await
        },
    )
    .await
}

/// Names a realm (`None` or empty removes the name).
#[tauri::command]
pub fn rename_instance(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
) -> Result<InstanceRecord> {
    state.launcher()?.rename(&id, name.as_deref()).map_err(ui)
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

/// The game started from the launcher: hide the window if the player wants.
fn game_started(app: &AppHandle) {
    if preferences::load(&rk_launcher::app_dir()).hide_while_playing {
        app.state::<AppState>()
            .hidden_for_game
            .store(true, Ordering::SeqCst);
        crate::tray::hide_main(app);
    }
}

/// A realm became ready while the launcher is out of sight (hidden in the
/// tray, minimised): a Windows notification says so (ADR 0020).
fn notify_ready_if_hidden(app: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let out_of_sight = !window.is_visible().unwrap_or(true)
        || window.is_minimized().unwrap_or(false)
        || !window.is_focused().unwrap_or(true);
    if !out_of_sight {
        return;
    }
    let body = app
        .state::<AppState>()
        .os_texts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ready
        .clone();
    let _ = app
        .notification()
        .builder()
        .title("RealmKeeper")
        .body(body)
        .show();
}

/// The game (or its session) ended: bring the window back if it was hidden.
fn game_ended(app: &AppHandle) {
    if app
        .state::<AppState>()
        .hidden_for_game
        .swap(false, Ordering::SeqCst)
    {
        crate::tray::show_main(app);
    }
}

/// After a clean stop: the daily backup when one is due (ADR 0018), in the
/// background; the next action on this instance waits for it.
fn schedule_daily_backup(app: &AppHandle, instance: &str) {
    let state = app.state::<AppState>();
    if state.closing.load(Ordering::SeqCst)
        || !preferences::load(&rk_launcher::app_dir()).daily_backup
    {
        return;
    }
    let Ok(launcher) = state.launcher() else {
        return;
    };
    let id = instance.to_owned();
    let handle = tauri::async_runtime::spawn(async move {
        // In the realm's turn: account work waits instead of borrowing the
        // database the backup started. Best effort: a failure leaves the
        // next stop to try again.
        let _turn = launcher.database_turn(&id).await;
        let _ = launcher.daily_backup(&id).await;
    });
    state
        .background
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(instance.to_owned(), handle);
}

/// Forwards session events in batches until the session ends; reacts to
/// the game starting or ending and to the end of the session.
fn forward_session_events(app: AppHandle, instance: String) -> rk_launcher::session::EventSink {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();
    let hooks = (app.clone(), instance.clone());
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
        let (app, instance) = &hooks;
        match &event {
            SessionEvent::Ready { .. } => notify_ready_if_hidden(app),
            SessionEvent::GameStarted { .. } => game_started(app),
            SessionEvent::GameExited { .. } => game_ended(app),
            SessionEvent::Ended { clean } => {
                update_shutdown_block(app, Some(instance));
                game_ended(app);
                if *clean {
                    schedule_daily_backup(app, instance);
                }
            }
            _ => {}
        }
        let _ = tx.send(event);
    })
}

/// Starts an instance; with `play`, prepares the client first and launches
/// the game once the servers are ready.
#[tauri::command]
pub async fn start_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    let launcher = state.launcher()?;
    let refused = |state: &AppState| -> Result<()> {
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
        Ok(())
    };
    refused(&state)?;
    // Held until the session is registered; a second start, or a change,
    // waiting for it then finds the realm running.
    let _turn = state.settle(&launcher, &id).await;
    refused(&state)?;
    let server = launcher.server(&id).map_err(ui)?;
    // The game is launched once the servers are online (`launch_game`).
    let events = forward_session_events(app.clone(), id.clone());
    let session = Session::start(server, None, events).map_err(ui)?;
    state
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, session);
    update_shutdown_block(&app, None);
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

/// Launches the game for an instance whose servers are running (or
/// starting: it is launched once they are ready).
#[tauri::command]
pub fn launch_game(state: State<'_, AppState>, id: String, keep_running: bool) -> Result<()> {
    let launcher = state.launcher()?;
    let sessions = state.sessions.lock().unwrap_or_else(|e| e.into_inner());
    let session = sessions
        .get(&id)
        .filter(|s| !s.is_finished())
        .ok_or_else(|| app_error("server.notRunning", &id))?;
    let (record, _) =
        rk_engine::install::open_existing(launcher.layout(), &id).map_err(|e| ui(e.into()))?;
    session.launch_game(launcher.prepare_client(&record).map_err(ui)?, keep_running);
    Ok(())
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

// --------------------------------------------------------- install location

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataLocation {
    /// Where realms, downloads and backups go.
    root: String,
    /// The default one (`%LOCALAPPDATA%\RealmKeeperData`).
    default: String,
    /// It can change to an empty folder: no realm is installed and
    /// `RK_ROOT` is not set.
    changeable: bool,
    /// It can point at another RealmKeeper folder (renamed or moved):
    /// `RK_ROOT` is not set.
    relocatable: bool,
    /// The chosen folder that no longer exists (renamed, moved, drive
    /// unplugged): the launcher waits for its new place.
    missing: Option<String>,
}

#[tauri::command]
pub fn data_location(state: State<'_, AppState>) -> Result<DataLocation> {
    let launcher = state.launcher()?;
    let relocatable = std::env::var_os("RK_ROOT").is_none();
    let changeable = relocatable && launcher.instances().map_err(ui)?.is_empty();
    let missing = rk_launcher::data_root()
        .missing
        .map(|p| p.display().to_string());
    Ok(DataLocation {
        root: launcher.layout().root().display().to_string(),
        default: rk_launcher::app_dir().display().to_string(),
        changeable,
        relocatable,
        missing,
    })
}

/// Sets the folder realms, downloads and backups live in: `folder` picked
/// by the player, used as is (`None` = back to the default). An empty
/// folder only while no realm is installed (nothing is moved); a RealmKeeper
/// folder (renamed or moved) at any time, servers stopped. The
/// launcher reopens on it.
#[tauri::command]
pub fn set_data_location(
    state: State<'_, AppState>,
    folder: Option<String>,
) -> Result<DataLocation> {
    let launcher = state.launcher()?;
    let folder = folder
        .map(PathBuf::from)
        .unwrap_or_else(rk_launcher::app_dir);
    let known = folder == rk_launcher::app_dir() || rk_launcher::is_data_root(&folder);
    if !known && !launcher.instances().map_err(ui)?.is_empty() {
        return Err(ui(LauncherError::LocationInUse));
    }
    if !state.running_ids().is_empty() {
        return Err(app_error("server.running", "location"));
    }
    if state
        .operation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
    {
        return Err(app_error("operation.busy", "location"));
    }
    rk_launcher::set_data_root(&folder).map_err(ui)?;
    let reopened = AppState::open_launcher(state.catalog.as_deref());
    *state.launcher.lock().unwrap_or_else(|e| e.into_inner()) = reopened;
    data_location(state)
}

// ------------------------------------------------------------ crash reports

/// Reports of earlier crashes, already redacted; the UI offers to open a
/// pre-filled issue (the player submits it, or not).
#[tauri::command]
pub fn crash_reports() -> Vec<rk_launcher::crash::CrashReport> {
    rk_launcher::crash::reports(&rk_launcher::app_dir())
}

#[tauri::command]
pub fn dismiss_crash_report(id: String) -> Result<()> {
    rk_launcher::crash::dismiss(&rk_launcher::app_dir(), &id).map_err(|e| app_error("unknown", e))
}

// ------------------------------------------------------------- diagnostic

/// A support report to paste into a bug report (personal folders redacted).
#[tauri::command]
pub async fn diagnostic_report(state: State<'_, AppState>) -> Result<String> {
    let launcher = state.launcher()?;
    let running = state.running_ids();
    tauri::async_runtime::spawn_blocking(move || {
        rk_launcher::diagnostic::report(&launcher, env!("CARGO_PKG_VERSION"), &running)
    })
    .await
    .map_err(|e| app_error("unknown", e))
}

// ------------------------------------------------------------- preferences

#[tauri::command]
pub fn preferences() -> Preferences {
    preferences::load(&rk_launcher::app_dir())
}

#[tauri::command]
pub fn set_preferences(preferences: Preferences) -> Result<Preferences> {
    preferences::save(&rk_launcher::app_dir(), &preferences)
        .map_err(|e| app_error("unknown", e))?;
    Ok(preferences)
}

/// The tray menu and what Windows shows for the launcher, in the player's
/// language.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_tray_texts(
    app: AppHandle,
    tray: State<'_, crate::tray::Tray>,
    state: State<'_, AppState>,
    show: String,
    stop: String,
    quit: String,
    shutdown: String,
    awake: String,
    ready: String,
) -> Result<()> {
    *state.os_texts.lock().unwrap_or_else(|e| e.into_inner()) = OsTexts {
        shutdown,
        awake,
        ready,
    };
    update_shutdown_block(&app, None);
    tray.set_texts(&show, &stop, &quit)
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
    /// Release notes by locale (`en`, `fr`): one line per change.
    notes: BTreeMap<String, Vec<String>>,
}

/// The notes of a feed entry: `notesByLocale` (written from
/// `app/src/lib/release-notes.json` by `tools/ci/app-release.mjs`), else
/// the plain `notes` text of older feeds, one change per line, as English.
fn feed_notes(raw: &serde_json::Value, body: Option<&str>) -> BTreeMap<String, Vec<String>> {
    let by_locale: Option<BTreeMap<String, Vec<String>>> = raw
        .get("notesByLocale")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    by_locale.unwrap_or_else(|| {
        let lines: Vec<String> = body
            .unwrap_or_default()
            .lines()
            .map(|l| {
                l.trim()
                    .trim_start_matches(['•', '-', '*'])
                    .trim()
                    .to_owned()
            })
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            BTreeMap::new()
        } else {
            BTreeMap::from([("en".to_owned(), lines)])
        }
    })
}

/// Download progress of a launcher update, emitted as `app-update`.
#[derive(Clone, Serialize)]
struct AppUpdateProgress {
    downloaded: u64,
    total: Option<u64>,
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
        notes: feed_notes(&u.raw_json, u.body.as_deref()),
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
    let mut downloaded = 0u64;
    let progress = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress.emit("app-update", AppUpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| app_error("appUpdate.failed", e))?;
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::feed_notes;

    #[test]
    fn feed_notes_by_locale_or_from_the_plain_text() {
        let raw = serde_json::json!({
            "notes": "• ignored",
            "notesByLocale": { "en": ["One."], "fr": ["Un."] }
        });
        let notes = feed_notes(&raw, Some("• ignored"));
        assert_eq!(notes["fr"], ["Un."]);
        assert_eq!(notes["en"], ["One."]);

        // A 0.3.x feed: one sentence, or one change per line.
        let old = feed_notes(&serde_json::json!({}), Some("• First.\n\n- Second.\n"));
        assert_eq!(old.len(), 1);
        assert_eq!(old["en"], ["First.", "Second."]);
        assert!(feed_notes(&serde_json::json!({}), None).is_empty());
    }
}

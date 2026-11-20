//! A running instance (and optionally the game) supervised in the
//! background: the app keeps a [`Session`] per instance, sends console lines
//! and a stop request, and receives [`SessionEvent`]s (progress, log lines,
//! game state). The CLI uses the same session with stdin and stderr.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use rk_runtime::{Server, ServerEvent, Stopped, Stream};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::{driver, LauncherError, PreparedClient, UiError};

/// What a session reports, in order. Serializable for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SessionEvent {
    DatabaseStarting,
    DatabaseReady,
    #[serde(rename_all = "camelCase")]
    ProcessStarting {
        process: String,
    },
    #[serde(rename_all = "camelCase")]
    ProcessReady {
        process: String,
        after_ms: u64,
    },
    /// Every process is ready: players can log in.
    #[serde(rename_all = "camelCase")]
    Ready {
        /// Address other computers use (LAN instances), else `127.0.0.1`.
        address: String,
        auth_port: u16,
    },
    #[serde(rename_all = "camelCase")]
    ProcessStopping {
        process: String,
    },
    #[serde(rename_all = "camelCase")]
    ProcessStopped {
        process: String,
        how: StopOutcome,
    },
    DatabaseStopped,
    Warning {
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Log {
        process: String,
        stderr: bool,
        text: String,
    },
    /// How far a process's start went (start progress bar, ADR 0020): its
    /// manifest `startup` milestones, as its output reaches them.
    #[serde(rename_all = "camelCase")]
    Startup {
        process: String,
        progress: f32,
        label: rk_core::LocalizedText,
    },
    /// Lines dropped because the receiver was too slow.
    #[serde(rename_all = "camelCase")]
    LogSkipped {
        lines: u64,
    },
    #[serde(rename_all = "camelCase")]
    GameStarted {
        /// The servers stay up when the game exits.
        keep_running: bool,
    },
    #[serde(rename_all = "camelCase")]
    GameExited {
        code: Option<i32>,
    },
    /// Characters in the world, every [`CENSUS_INTERVAL`] while the servers
    /// run (emulators whose driver can tell).
    #[serde(rename_all = "camelCase")]
    Population {
        players: u32,
        bots: u32,
    },
    /// A process ended by itself (crash, or a shutdown typed in its console).
    #[serde(rename_all = "camelCase")]
    ProcessExited {
        process: String,
        code: Option<i32>,
    },
    /// The session failed (start error, game launch error).
    Failed {
        error: UiError,
    },
    /// Last event: everything is stopped.
    #[serde(rename_all = "camelCase")]
    Ended {
        /// `false` after a failure or an unexpected process exit.
        clean: bool,
    },
}

impl SessionEvent {
    fn from_server(event: ServerEvent) -> Self {
        match event {
            ServerEvent::DatabaseStarting => Self::DatabaseStarting,
            ServerEvent::DatabaseReady => Self::DatabaseReady,
            ServerEvent::Starting { process } => Self::ProcessStarting { process },
            ServerEvent::Ready { process, after } => Self::ProcessReady {
                process,
                after_ms: after.as_millis().try_into().unwrap_or(u64::MAX),
            },
            ServerEvent::Stopping { process } => Self::ProcessStopping { process },
            ServerEvent::Stopped { process, how } => Self::ProcessStopped {
                process,
                how: match how {
                    Stopped::Clean => StopOutcome::Clean,
                    Stopped::Killed => StopOutcome::Killed,
                    Stopped::AlreadyExited => StopOutcome::AlreadyExited,
                },
            },
            ServerEvent::DatabaseStopped => Self::DatabaseStopped,
            ServerEvent::Warning(message) => Self::Warning { message },
        }
    }
}

/// How a process stop went (see [`Stopped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StopOutcome {
    Clean,
    Killed,
    AlreadyExited,
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// How often a running session counts the characters in the world (one
/// small query; bots log in over minutes after the start).
pub const CENSUS_INTERVAL: Duration = Duration::from_secs(20);
/// Longest a count may take before it is given up.
const CENSUS_TIMEOUT: Duration = Duration::from_secs(5);

/// The game to launch once the servers are ready.
pub struct Game {
    pub client: PreparedClient,
    /// Keep the servers up when the game exits.
    pub keep_running: bool,
}

enum Command {
    Console(String),
    /// Launches the game in a session whose servers are up.
    Launch(Launch),
    /// Work that needs the servers up: run (once the players are saved when
    /// asked: a live backup); a stop waits for it.
    WhileRunning {
        work: Work,
        save_players: bool,
    },
    /// A console line whose reply is gathered (ADR 0025).
    Ask(Ask),
    Stop,
}

/// A console request: the line, how its reply lines look and end.
struct Ask {
    line: String,
    marker: &'static str,
    last: fn(&str) -> bool,
    timeout: Duration,
    answer: oneshot::Sender<Option<Vec<String>>>,
}

/// [`SessionControl::while_running`] work, its result sent on by itself.
type Work = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A game to launch in a running session.
struct Launch {
    client: Box<PreparedClient>,
    /// Keep the servers up when this game exits.
    keep_running: bool,
}

/// Sends commands to a session; cheap to clone.
#[derive(Clone)]
pub struct SessionControl {
    commands: mpsc::UnboundedSender<Command>,
}

impl SessionControl {
    /// Sends a line to the server console (worldserver).
    pub fn console(&self, line: &str) {
        let _ = self.commands.send(Command::Console(line.to_owned()));
    }

    /// Sends a console line and returns the reply lines holding `marker`,
    /// up to the one `last` accepts (ADR 0025). Requests run one after the
    /// other. `None` when the servers are not ready, the line could not be
    /// sent or the reply did not end within `timeout`.
    pub async fn ask(
        &self,
        line: &str,
        marker: &'static str,
        last: fn(&str) -> bool,
        timeout: Duration,
    ) -> Option<Vec<String>> {
        let (answer, answered) = oneshot::channel();
        self.commands
            .send(Command::Ask(Ask {
                line: line.to_owned(),
                marker,
                last,
                timeout,
                answer,
            }))
            .ok()?;
        answered.await.ok().flatten()
    }

    /// Asks for a clean stop; [`Session::wait`] returns once it is done.
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    /// Launches the game once the servers are ready (at once if they are).
    /// When it exits the servers stop, unless `keep_running`.
    pub fn launch_game(&self, client: PreparedClient, keep_running: bool) {
        let _ = self.commands.send(Command::Launch(Launch {
            client: Box::new(client),
            keep_running,
        }));
    }

    /// Runs `work` while the servers stay up (a backup of the running
    /// database, ADR 0022): the players are saved first (emulator
    /// `savePlayers`), a stop requested meanwhile waits for it, crashes are
    /// still reported. `None` when the servers are not ready (starting,
    /// stopping, ended) or another such work runs: `work` is not run.
    pub async fn while_running<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
    ) -> Option<T> {
        self.exclusive(work, true).await
    }

    /// Runs short database work (accounts) while the servers stay up:
    /// players not saved, beside a live backup if one runs. A stop waits
    /// for it, so its database never goes away under it. `None` at once
    /// when the servers are not ready (starting, stopping, ended).
    pub async fn with_database<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
    ) -> Option<T> {
        self.exclusive(work, false).await
    }

    async fn exclusive<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
        save_players: bool,
    ) -> Option<T> {
        let (answer, answered) = oneshot::channel();
        let work: Work = Box::pin(async move {
            let _ = answer.send(work.await);
        });
        self.commands
            .send(Command::WhileRunning { work, save_players })
            .ok()?;
        answered.await.ok()
    }
}

/// Handle of a running session. Once it and every [`SessionControl`] are
/// dropped, the session stops cleanly by itself.
pub struct Session {
    instance: String,
    control: SessionControl,
    task: JoinHandle<bool>,
}

impl Session {
    /// Starts the instance in a background task (needs a tokio runtime).
    pub fn start(
        server: Server,
        game: Option<Game>,
        events: EventSink,
    ) -> Result<Self, LauncherError> {
        let driver = driver(server.driver_id())?;
        let instance = server.record().id.clone();
        let (commands, receiver) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(Arc::new(server), driver, game, events, receiver));
        Ok(Self {
            instance,
            control: SessionControl { commands },
            task,
        })
    }

    pub fn instance(&self) -> &str {
        &self.instance
    }

    pub fn control(&self) -> SessionControl {
        self.control.clone()
    }

    /// Whether the session task has ended.
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub fn console(&self, line: &str) {
        self.control.console(line);
    }

    pub fn stop(&self) {
        self.control.stop();
    }

    pub fn launch_game(&self, client: PreparedClient, keep_running: bool) {
        self.control.launch_game(client, keep_running);
    }

    /// Waits for the end; `true` when it ended cleanly.
    pub async fn wait(self) -> bool {
        let Self { control, task, .. } = self;
        // Keep the channel open while waiting: closing it means "stop".
        let clean = task.await.unwrap_or(false);
        drop(control);
        clean
    }
}

async fn run(
    server: Arc<Server>,
    driver: Arc<dyn rk_runtime::EmulatorDriver>,
    game: Option<Game>,
    events: EventSink,
    mut commands: mpsc::UnboundedReceiver<Command>,
) -> bool {
    let mut logs = server.subscribe();
    let log_events = events.clone();
    let mut startup = StartupTracker::new(server.emulator());
    let forward = tokio::spawn(async move {
        loop {
            match logs.recv().await {
                Ok(line) => {
                    if let Some(event) = startup.see(&line.process, &line.text) {
                        log_events(event);
                    }
                    log_events(SessionEvent::Log {
                        process: line.process,
                        stderr: line.stream == Stream::Stderr,
                        text: line.text,
                    })
                }
                Err(broadcast::error::RecvError::Lagged(lines)) => {
                    log_events(SessionEvent::LogSkipped { lines })
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let observer_events = events.clone();
    let observer = move |event: ServerEvent| observer_events(SessionEvent::from_server(event));

    let clean = supervise(
        &server,
        driver.as_ref(),
        game,
        &events,
        &observer,
        &mut commands,
    )
    .await;
    // Let the last log lines through before the final event.
    tokio::task::yield_now().await;
    forward.abort();
    events(SessionEvent::Ended { clean });
    clean
}

/// Follows the start milestones of each process: only a milestone further
/// than the last one seen counts (a line never seen is skipped).
struct StartupTracker {
    milestones: Vec<(String, Vec<rk_core::manifest::StartupMilestone>)>,
    reached: std::collections::BTreeMap<String, usize>,
}

impl StartupTracker {
    fn new(emulator: &rk_core::manifest::EmulatorManifest) -> Self {
        Self {
            milestones: emulator
                .processes
                .iter()
                .filter(|p| !p.startup.is_empty())
                .map(|p| (p.id.clone(), p.startup.clone()))
                .collect(),
            reached: Default::default(),
        }
    }

    fn see(&mut self, process: &str, text: &str) -> Option<SessionEvent> {
        let (_, milestones) = self.milestones.iter().find(|(id, _)| id == process)?;
        let next = self.reached.get(process).map_or(0, |i| i + 1);
        let found = milestones[next.min(milestones.len())..]
            .iter()
            .position(|m| text.contains(&m.pattern))?
            + next;
        self.reached.insert(process.to_owned(), found);
        let milestone = &milestones[found];
        Some(SessionEvent::Startup {
            process: process.to_owned(),
            progress: milestone.progress,
            label: milestone.label.clone(),
        })
    }
}

async fn supervise(
    server: &Arc<Server>,
    driver: &dyn rk_runtime::EmulatorDriver,
    game: Option<Game>,
    events: &EventSink,
    observer: &(dyn Fn(ServerEvent) + Send + Sync),
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> bool {
    let fail = |error: LauncherError| {
        events(SessionEvent::Failed {
            error: error.to_ui(),
        })
    };
    // A stop requested during the start stops what was started, cleanly; a
    // game asked for meanwhile is launched once the servers are ready.
    let mut queued: Option<Launch> = None;
    let running = match server
        .start_unless(driver, observer, wait_stop(commands, &mut queued))
        .await
    {
        Ok(Some(running)) => running,
        Ok(None) => return true,
        Err(error) => {
            fail(error.into());
            return false;
        }
    };
    let record = server.record();
    let address = match record.network {
        rk_engine::install::Network::Solo => "127.0.0.1".to_owned(),
        rk_engine::install::Network::Lan => rk_runtime::lan_ipv4()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "127.0.0.1".into()),
    };
    events(SessionEvent::Ready {
        address,
        auth_port: record.ports.get("auth").copied().unwrap_or_default(),
    });

    let mut keep_running = game.as_ref().is_none_or(|g| g.keep_running);
    // Held while its game runs: the client stays locked.
    let mut prepared: Option<Box<PreparedClient>> = None;
    let mut child = None;
    if let Some(game) = game {
        match rk_client::launch(&game.client.manifest, &game.client.state) {
            Ok(c) => {
                events(SessionEvent::GameStarted { keep_running });
                child = Some(c);
                prepared = Some(Box::new(game.client));
            }
            Err(error) => {
                fail(error.into());
                running.stop(observer).await;
                return false;
            }
        }
    } else if let Some(launch) = queued.take() {
        keep_running = launch.keep_running;
        launch_later(launch, &mut child, &mut prepared, events);
    }

    let mut census = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(3),
        CENSUS_INTERVAL,
    );
    census.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut clean = true;
    // Work that needs the servers up: at most one that saves the players
    // first (a live backup), any number of short ones (accounts) beside it.
    // A stop waits for all of them; once it is asked, new work is refused
    // at once.
    let mut exclusive: Option<Pin<Box<dyn Future<Output = ()> + Send + '_>>> = None;
    let mut quick = tokio::task::JoinSet::new();
    // Console requests: one at a time (their replies would mix), in order;
    // a stop or a crash drops them (their callers get `None`).
    let mut asks: std::collections::VecDeque<Ask> = std::collections::VecDeque::new();
    let mut asking: Option<Pin<Box<dyn Future<Output = ()> + Send + '_>>> = None;
    let mut stop_after = false;
    // Every control dropped: nothing more to read.
    let mut closed = false;
    loop {
        if asking.is_none() {
            if let Some(ask) = asks.pop_front() {
                let running = &running;
                asking = Some(Box::pin(async move {
                    let reply = running
                        .ask(&ask.line, ask.marker, ask.last, ask.timeout)
                        .await;
                    let _ = ask.answer.send(reply);
                }));
            }
        }
        tokio::select! {
            () = async { asking.as_mut().expect("guarded").await }, if asking.is_some() => {
                asking = None;
            }
            () = async { exclusive.as_mut().expect("guarded").await }, if exclusive.is_some() => {
                exclusive = None;
                if stop_after && quick.is_empty() {
                    break;
                }
            }
            Some(_) = quick.join_next(), if !quick.is_empty() => {
                if stop_after && exclusive.is_none() && quick.is_empty() {
                    break;
                }
            }
            _ = census.tick() => {
                // Best effort: a failed or slow count only leaves the figures
                // as they were (bounded: a stop or a crash waits for it).
                let count = tokio::time::timeout(CENSUS_TIMEOUT, server.population(driver)).await;
                if let Ok(Ok(Some(count))) = count {
                    events(SessionEvent::Population { players: count.players, bots: count.bots });
                }
            }
            command = commands.recv(), if !closed => match command {
                None | Some(Command::Stop) => {
                    closed |= command.is_none();
                    if exclusive.is_some() || !quick.is_empty() {
                        stop_after = true;
                    } else {
                        break;
                    }
                }
                // A refused work is dropped: its caller gets `None` at once.
                Some(Command::WhileRunning { .. }) if stop_after => {}
                Some(Command::WhileRunning { work, save_players: false }) => {
                    quick.spawn(work);
                }
                Some(Command::WhileRunning { work, save_players: true }) => {
                    // One at a time.
                    if exclusive.is_none() {
                        let running = &running;
                        exclusive = Some(Box::pin(async move {
                            running.save_players().await;
                            work.await;
                        }));
                    }
                }
                Some(Command::Launch(_)) if stop_after => {
                    events(SessionEvent::Warning { message: "the servers are stopping".into() });
                }
                Some(Command::Launch(launch)) => {
                    if child.is_some() {
                        events(SessionEvent::Warning { message: "the game is already running".into() });
                    } else {
                        keep_running = launch.keep_running;
                        launch_later(launch, &mut child, &mut prepared, events);
                    }
                }
                // Refused once a stop is asked: its caller gets `None`.
                Some(Command::Ask(_)) if stop_after => {}
                Some(Command::Ask(ask)) => asks.push_back(ask),
                Some(Command::Console(line)) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match running.console() {
                        Some(console) => {
                            if let Err(error) = console.send_line(line).await {
                                events(SessionEvent::Warning { message: format!("console: {error}") });
                            }
                        }
                        None => events(SessionEvent::Warning {
                            message: "this server has no console".into(),
                        }),
                    }
                }
            },
            (process, exit) = running.wait_any_exit() => {
                events(SessionEvent::ProcessExited { process, code: exit.code });
                clean = false;
                break;
            }
            status = async {
                match child.as_mut() {
                    Some(c) => c.wait().await,
                    None => std::future::pending().await,
                }
            } => {
                child = None;
                prepared = None;
                events(SessionEvent::GameExited { code: status.ok().and_then(|s| s.code()) });
                if !keep_running {
                    if exclusive.is_some() || !quick.is_empty() {
                        stop_after = true;
                    } else {
                        break;
                    }
                }
            }
        }
    }
    // A crash ends a work in progress (its database may go away).
    drop(exclusive);
    drop(asking);
    drop(asks);
    quick.abort_all();
    // Work asked for from now on is refused at once, not after the stop.
    commands.close();
    while commands.try_recv().is_ok() {}
    running.stop(observer).await;
    drop(prepared);
    clean
}

/// Launches the game in a running session; a failure is reported, the
/// servers keep running.
fn launch_later(
    launch: Launch,
    child: &mut Option<tokio::process::Child>,
    prepared: &mut Option<Box<PreparedClient>>,
    events: &EventSink,
) {
    let Launch {
        client,
        keep_running,
    } = launch;
    match rk_client::launch(&client.manifest, &client.state) {
        Ok(c) => {
            events(SessionEvent::GameStarted { keep_running });
            *child = Some(c);
            *prepared = Some(client);
        }
        Err(error) => events(SessionEvent::Failed {
            error: LauncherError::from(error).to_ui(),
        }),
    }
}

/// Waits for a stop request during the start; a game launch asked for
/// meanwhile is kept for when the servers are ready.
async fn wait_stop(commands: &mut mpsc::UnboundedReceiver<Command>, queued: &mut Option<Launch>) {
    loop {
        match commands.recv().await {
            None | Some(Command::Stop) => return,
            Some(Command::Launch(launch)) => *queued = Some(launch),
            Some(Command::Console(_)) => {}
            // Not ready yet: refused (dropped, its caller gets `None`).
            Some(Command::Ask(_)) => {}
            // Not ready yet: refused (dropped, its caller gets `None`).
            Some(Command::WhileRunning { .. }) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_milestones_only_move_forward() {
        let catalog = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let catalog = rk_catalog::load_dir(&catalog).unwrap();
        let emulator = &catalog.emulators["azerothcore"].manifest;
        let mut tracker = StartupTracker::new(emulator);
        let progress = |e: Option<SessionEvent>| match e {
            Some(SessionEvent::Startup { progress, .. }) => Some(progress),
            _ => None,
        };
        assert_eq!(
            progress(tracker.see("authserver", "Loading Quests...")),
            None
        );
        let spells = progress(tracker.see("worldserver", "Loading SpellInfo Store...")).unwrap();
        // An earlier milestone seen later is ignored; a later one counts.
        assert_eq!(
            progress(tracker.see("worldserver", "Loading World Information...")),
            None
        );
        let bots = progress(tracker.see("worldserver", "Load Playerbots Config...")).unwrap();
        assert!(bots > spells);
        assert_eq!(
            progress(tracker.see("worldserver", "Load Playerbots Config...")),
            None
        );
        assert_eq!(progress(tracker.see("worldserver", "unrelated line")), None);
    }
}

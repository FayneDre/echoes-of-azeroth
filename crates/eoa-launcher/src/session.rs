//! A running instance (and optionally the game) supervised in the
//! background: the app keeps a [`Session`] per instance, sends console lines
//! and a stop request, and receives [`SessionEvent`]s (progress, log lines,
//! game state). The CLI uses the same session with stdin and stderr.

use std::sync::Arc;

use eoa_runtime::{Server, ServerEvent, Stopped, Stream};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
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
        killed: bool,
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
    /// Lines dropped because the receiver was too slow.
    #[serde(rename_all = "camelCase")]
    LogSkipped {
        lines: u64,
    },
    GameStarted,
    #[serde(rename_all = "camelCase")]
    GameExited {
        code: Option<i32>,
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
                killed: how == Stopped::Killed,
            },
            ServerEvent::DatabaseStopped => Self::DatabaseStopped,
            ServerEvent::Warning(message) => Self::Warning { message },
        }
    }
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// The game to launch once the servers are ready.
pub struct Game {
    pub client: PreparedClient,
    /// Keep the servers up when the game exits.
    pub keep_running: bool,
}

enum Command {
    Console(String),
    Stop,
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

    /// Asks for a clean stop; [`Session::wait`] returns once it is done.
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
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
    driver: Arc<dyn eoa_runtime::EmulatorDriver>,
    game: Option<Game>,
    events: EventSink,
    mut commands: mpsc::UnboundedReceiver<Command>,
) -> bool {
    let mut logs = server.subscribe();
    let log_events = events.clone();
    let forward = tokio::spawn(async move {
        loop {
            match logs.recv().await {
                Ok(line) => log_events(SessionEvent::Log {
                    process: line.process,
                    stderr: line.stream == Stream::Stderr,
                    text: line.text,
                }),
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

async fn supervise(
    server: &Arc<Server>,
    driver: &dyn eoa_runtime::EmulatorDriver,
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
    // A stop requested during the start stops what was started, cleanly.
    let running = match server
        .start_unless(driver, observer, wait_stop(commands))
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
        eoa_engine::install::Network::Solo => "127.0.0.1".to_owned(),
        eoa_engine::install::Network::Lan => eoa_runtime::lan_ipv4()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "127.0.0.1".into()),
    };
    events(SessionEvent::Ready {
        address,
        auth_port: record.ports.get("auth").copied().unwrap_or_default(),
    });

    let keep_running = game.as_ref().is_none_or(|g| g.keep_running);
    // Held until the end: the client stays locked while its game runs.
    let mut prepared = None;
    let mut child = None;
    if let Some(game) = game {
        match eoa_client::launch(&game.client.manifest, &game.client.state) {
            Ok(c) => {
                events(SessionEvent::GameStarted);
                child = Some(c);
                prepared = Some(game.client);
            }
            Err(error) => {
                fail(error.into());
                running.stop(observer).await;
                return false;
            }
        }
    }

    let mut clean = true;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                None | Some(Command::Stop) => break,
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
                    break;
                }
            }
        }
    }
    running.stop(observer).await;
    drop(prepared);
    clean
}

async fn wait_stop(commands: &mut mpsc::UnboundedReceiver<Command>) {
    loop {
        match commands.recv().await {
            None | Some(Command::Stop) => return,
            Some(Command::Console(_)) => {}
        }
    }
}

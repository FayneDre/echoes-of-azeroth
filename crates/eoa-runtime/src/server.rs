//! A whole instance: database, then the emulator's processes in manifest
//! order, each gated by its readiness; stopped in reverse order.

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use eoa_core::manifest::{EmulatorManifest, Readiness};
use eoa_core::Catalog;
use eoa_engine::install::{self, InstanceRecord, Layout, Network};
use futures_util::stream::{FuturesUnordered, StreamExt};
use regex::Regex;
use tokio::sync::broadcast;

use crate::driver::{DriverContext, EmulatorDriver, NewAccount};
use crate::process::{Exit, LogLine, Process, ProcessConfig, Ready, Stopped};
use crate::RuntimeError;

/// Startup budget of one process. Generous on purpose: a first world start
/// with hundreds of bots takes minutes, and an early exit is caught at once.
const START_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// Progress of [`Server::start`] / [`Running::stop`], for the UI and CLI.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    DatabaseStarting,
    DatabaseReady,
    Starting {
        process: String,
    },
    Ready {
        process: String,
        after: Duration,
    },
    Stopping {
        process: String,
    },
    Stopped {
        process: String,
        how: Stopped,
    },
    DatabaseStopped,
    /// Something worked around, worth telling the player.
    Warning(String),
}

pub type Observer<'a> = &'a (dyn Fn(ServerEvent) + Send + Sync);

/// An installed instance, ready to be started.
pub struct Server {
    layout: Layout,
    db: eoa_db::Instance,
    emulator: EmulatorManifest,
    record: InstanceRecord,
    processes: Vec<ProcessConfig>,
    logs: broadcast::Sender<LogLine>,
}

impl Server {
    /// Reads an installed instance and plans its processes from the manifest.
    pub fn open(catalog: &Catalog, layout: &Layout, id: &str) -> Result<Self, RuntimeError> {
        let (record, _) = install::open_existing(layout, id)?;
        let emulator = catalog
            .emulator(&record.emulator)
            .ok_or_else(|| install::PlanError::Unknown(format!("emulator '{}'", record.emulator)))?
            .manifest
            .clone();
        let db = install::database(catalog, layout, &record)?;
        let paths = layout.instance(id);
        let processes = emulator
            .processes
            .iter()
            .map(|spec| {
                let ready = match &spec.readiness {
                    Readiness::Tcp { port } => {
                        Ready::Tcp(*record.ports.get(port).ok_or_else(|| {
                            RuntimeError::UnknownPort {
                                process: spec.id.clone(),
                                port: port.clone(),
                            }
                        })?)
                    }
                    Readiness::LogLine { pattern } => {
                        Ready::LogLine(Regex::new(pattern).map_err(|e| RuntimeError::Pattern {
                            process: spec.id.clone(),
                            message: e.to_string(),
                        })?)
                    }
                };
                Ok(ProcessConfig {
                    id: spec.id.clone(),
                    program: paths.root.join(&spec.executable),
                    args: spec.args.clone(),
                    cwd: paths.root.clone(),
                    log_file: paths.logs.join(format!("{}.console.log", spec.id)),
                    ready,
                    stop: spec.stop.clone(),
                    start_timeout: START_TIMEOUT,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let (logs, _) = broadcast::channel(4096);
        Ok(Self {
            layout: layout.clone(),
            db,
            emulator,
            record,
            processes,
            logs,
        })
    }

    pub fn record(&self) -> &InstanceRecord {
        &self.record
    }

    /// The driver this instance needs (emulator manifest `driver`).
    pub fn driver_id(&self) -> &str {
        &self.emulator.driver
    }

    pub fn db(&self) -> &eoa_db::Instance {
        &self.db
    }

    /// Output of every process, line by line.
    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.logs.subscribe()
    }

    /// Address the client connects to, and a warning when LAN mode could not
    /// find a LAN address.
    fn realm_address(&self) -> (Ipv4Addr, Option<String>) {
        match self.record.network {
            Network::Solo => (Ipv4Addr::LOCALHOST, None),
            Network::Lan => match crate::network::lan_ipv4() {
                Some(ip) => (ip, None),
                None => (
                    Ipv4Addr::LOCALHOST,
                    Some("no local network address found: only this computer can connect".into()),
                ),
            },
        }
    }

    fn context(&self, realm_address: Ipv4Addr) -> DriverContext<'_> {
        DriverContext {
            db: &self.db,
            emulator: &self.emulator,
            record: &self.record,
            realm_address,
        }
    }

    fn check_driver(&self, driver: &dyn EmulatorDriver) -> Result<(), RuntimeError> {
        if driver.id() == self.emulator.driver {
            Ok(())
        } else {
            Err(RuntimeError::Driver {
                driver: driver.id().into(),
                source: format!("this instance needs the '{}' driver", self.emulator.driver).into(),
            })
        }
    }

    /// Starts the database (if needed), lets the driver prepare, then starts
    /// every process in order. On failure, what was started is stopped again.
    pub async fn start(
        self: &Arc<Self>,
        driver: &dyn EmulatorDriver,
        observer: Observer<'_>,
    ) -> Result<Running, RuntimeError> {
        let running = self
            .start_unless(driver, observer, std::future::pending())
            .await?;
        Ok(running.expect("never cancelled"))
    }

    /// Like [`Server::start`], but gives up when `stop` completes first:
    /// what was started (processes and database) is then stopped cleanly
    /// and `None` is returned.
    pub async fn start_unless(
        self: &Arc<Self>,
        driver: &dyn EmulatorDriver,
        observer: Observer<'_>,
        stop: impl std::future::Future<Output = ()>,
    ) -> Result<Option<Running>, RuntimeError> {
        self.check_driver(driver)?;
        let id = &self.record.id;
        let lock = install::lock_instance(&self.layout, id)?;
        // A half-applied install, update or module change is resumed first.
        install::ensure_finished(
            &self.layout,
            &[
                install::instance_operation_id(id),
                install::modules_operation_id(id),
            ],
        )?;
        observer(ServerEvent::DatabaseStarting);
        self.db.start().await?;
        observer(ServerEvent::DatabaseReady);
        let mut running = Running {
            server: self.clone(),
            processes: Vec::new(),
            _lock: lock,
        };
        // The processes are recorded in `running` as soon as they are
        // spawned, so a stop during a readiness wait stops them too.
        let started = tokio::select! {
            started = self.start_processes(driver, observer, &mut running) => Some(started),
            () = stop => None,
        };
        match started {
            Some(Ok(())) => Ok(Some(running)),
            Some(Err(error)) => {
                running.stop(observer).await;
                Err(error)
            }
            None => {
                running.stop(observer).await;
                Ok(None)
            }
        }
    }

    async fn start_processes(
        &self,
        driver: &dyn EmulatorDriver,
        observer: Observer<'_>,
        running: &mut Running,
    ) -> Result<(), RuntimeError> {
        let (address, warning) = self.realm_address();
        if let Some(warning) = warning {
            observer(ServerEvent::Warning(warning));
        }
        driver
            .prepare_start(&self.context(address))
            .await
            .map_err(|source| RuntimeError::Driver {
                driver: driver.id().into(),
                source,
            })?;
        for config in &self.processes {
            observer(ServerEvent::Starting {
                process: config.id.clone(),
            });
            let process = Process::spawn(config.clone(), self.logs.clone()).await?;
            running.processes.push(process);
            let process = running.processes.last().expect("just pushed");
            let after = process.wait_ready().await?;
            observer(ServerEvent::Ready {
                process: config.id.clone(),
                after,
            });
        }
        Ok(())
    }

    /// Creates a game account. Starts the database for it when it is not
    /// running, and stops it again afterwards.
    pub async fn create_account(
        &self,
        driver: &dyn EmulatorDriver,
        account: &NewAccount,
    ) -> Result<u32, RuntimeError> {
        self.check_driver(driver)?;
        let was_running = self.db.ping().await;
        if !was_running {
            self.db.start().await?;
        }
        let (address, _) = self.realm_address();
        let created = driver
            .create_account(&self.context(address), account)
            .await
            .map_err(|source| RuntimeError::Driver {
                driver: driver.id().into(),
                source,
            });
        if !was_running {
            self.db.stop().await?;
        }
        created
    }
}

/// A started instance. Dropping it without [`stop`](Self::stop) still ends
/// the servers (stdin closes, job objects kill), but leaves MySQL running
/// until the next start or stop adopts it.
pub struct Running {
    server: Arc<Server>,
    processes: Vec<Process>,
    /// Released once the processes are stopped (dropped after them).
    _lock: install::InstanceLock,
}

impl Running {
    pub fn server(&self) -> &Arc<Server> {
        &self.server
    }

    pub fn process(&self, id: &str) -> Option<&Process> {
        self.processes.iter().find(|p| p.id() == id)
    }

    /// The process that takes console commands: the first one stopped by
    /// closing stdin, since its stdin is a command line.
    pub fn console(&self) -> Option<&Process> {
        self.processes.iter().find(|p| p.takes_console())
    }

    /// Waits until any process exits (a crash, or a shutdown typed in its
    /// console) and returns which one.
    pub async fn wait_any_exit(&self) -> (String, Exit) {
        let mut exits: FuturesUnordered<_> = self
            .processes
            .iter()
            .map(|p| async move { (p.id().to_owned(), p.wait().await) })
            .collect();
        match exits.next().await {
            Some(exit) => exit,
            None => std::future::pending().await,
        }
    }

    /// Stops the processes in reverse order, then the database.
    pub async fn stop(mut self, observer: Observer<'_>) {
        while let Some(process) = self.processes.pop() {
            observer(ServerEvent::Stopping {
                process: process.id().into(),
            });
            let how = process.stop().await;
            observer(ServerEvent::Stopped {
                process: process.id().into(),
                how,
            });
        }
        match self.server.db.stop().await {
            Ok(()) => observer(ServerEvent::DatabaseStopped),
            Err(error) => observer(ServerEvent::Warning(format!(
                "the database did not stop cleanly: {error}"
            ))),
        }
    }
}

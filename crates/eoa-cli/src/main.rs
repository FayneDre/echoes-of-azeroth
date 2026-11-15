//! `eoa`: headless entry point to the same crates the launcher uses.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use eoa_engine::install::{self, Layout};

#[derive(Parser)]
#[command(
    name = "eoa",
    version,
    about = "Echoes of Azeroth command-line interface"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Catalog operations.
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    /// Content-addressed store operations.
    Store {
        #[command(subcommand)]
        command: StoreCommand,
    },
    /// Create a server instance from the catalog and a server build (resumable).
    Install {
        /// Instance id: lowercase letters, digits and '-'.
        id: String,
        #[arg(long, default_value = "azerothcore")]
        emulator: String,
        /// Defaults to the emulator's default flavor.
        #[arg(long)]
        flavor: Option<String>,
        /// Optional module to enable (repeatable). Mandatory modules are always added.
        #[arg(long = "module")]
        modules: Vec<String>,
        /// Listen on the local network instead of this computer only.
        #[arg(long)]
        lan: bool,
        /// Server build ZIP (CI artifact with `dist/` and `sql/`). Without it,
        /// the build is found in the signed index (`--index`).
        #[arg(long, conflicts_with = "index")]
        server: Option<PathBuf>,
        /// Database snapshot ZIP made by CI for this server build (ADR 0009):
        /// restores the databases instead of importing them from empty.
        #[arg(long, requires = "server")]
        db_snapshot: Option<PathBuf>,
        /// Signed index (file or URL, ADR 0010) to find the build in.
        /// Defaults to the project's published index.
        #[arg(long)]
        index: Option<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        /// Data root (`%LOCALAPPDATA%EchoesOfAzeroth` for the launcher).
        #[arg(long, default_value = ".local/eoa")]
        root: PathBuf,
    },
    /// Signed artifact index (ADR 0010).
    Index {
        #[command(subcommand)]
        command: IndexCommand,
    },
    /// Shared third-party runtimes (MySQL...).
    Component {
        #[command(subcommand)]
        command: ComponentCommand,
    },
    /// Start an installed instance in the foreground. Lines typed are sent to
    /// the server console; Ctrl+C, `.stop` or end of input stops everything cleanly.
    Start {
        id: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        /// Data root (`%LOCALAPPDATA%EchoesOfAzeroth` for the launcher).
        #[arg(long, default_value = ".local/eoa")]
        root: PathBuf,
    },
    /// Game accounts of an instance.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
}

#[derive(Subcommand)]
enum AccountCommand {
    /// Create a game account (the database is started for it if needed).
    Create {
        /// Instance id.
        instance: String,
        username: String,
        /// Read from the first line of stdin when omitted.
        #[arg(long)]
        password: Option<String>,
        /// GM level: 0 player, 1 moderator, 2 game master, 3 administrator.
        #[arg(long, default_value_t = 0)]
        gm: u8,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/eoa")]
        root: PathBuf,
    },
}

/// The driver named by an emulator manifest.
fn driver(name: &str) -> Result<Box<dyn eoa_runtime::EmulatorDriver>> {
    match name {
        "azerothcore" => Ok(Box::new(eoa_driver_azerothcore::AzerothCore)),
        other => anyhow::bail!("no driver '{other}' in this build"),
    }
}

/// Prints supervisor progress.
fn print_event(event: eoa_runtime::ServerEvent) {
    use eoa_runtime::ServerEvent as E;
    match event {
        E::DatabaseStarting => eprintln!("== starting the database"),
        E::DatabaseReady => eprintln!("== database ready"),
        E::Starting { process } => eprintln!("== starting {process}"),
        E::Ready { process, after } => {
            eprintln!("== {process} ready after {:.1} s", after.as_secs_f64())
        }
        E::Stopping { process } => eprintln!("== stopping {process}"),
        E::Stopped { process, how } => eprintln!("== {process} stopped ({how:?})"),
        E::DatabaseStopped => eprintln!("== database stopped"),
        E::Warning(message) => eprintln!("warning: {message}"),
    }
}

/// `eoa start`: runs until Ctrl+C, `.stop`, end of input or a server exit.
async fn run_instance(server: eoa_runtime::Server) -> Result<ExitCode> {
    let server = Arc::new(server);
    let driver = driver(server.driver_id())?;
    let mut logs = server.subscribe();
    tokio::spawn(async move {
        loop {
            match logs.recv().await {
                Ok(line) => println!("[{}] {}", line.process, line.text),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("({n} lines skipped)")
                }
                Err(_) => break,
            }
        }
    });
    let running = server.start(driver.as_ref(), &print_event).await?;
    let record = server.record();
    eprintln!(
        "== ready: set your client's realmlist to {} (auth port {}). \
         Type console commands; Ctrl+C or .stop to stop.",
        match record.network {
            install::Network::Solo => "127.0.0.1".to_owned(),
            install::Network::Lan => eoa_runtime::lan_ipv4()
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "127.0.0.1".into()),
        },
        record.ports.get("auth").copied().unwrap_or_default()
    );

    // stdin is read on a plain thread: tokio's stdin cannot be interrupted.
    let (tx, mut lines) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut code = ExitCode::SUCCESS;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            line = lines.recv() => match line.as_deref().map(str::trim) {
                None | Some(".stop") => break,
                Some("") => {}
                Some(command) => match running.console() {
                    Some(console) => {
                        if let Err(error) = console.send_line(command).await {
                            eprintln!("console: {error}");
                        }
                    }
                    None => eprintln!("this server has no console"),
                },
            },
            (process, exit) = running.wait_any_exit() => {
                eprintln!("== {process} exited (code {:?}); stopping the others", exit.code);
                code = ExitCode::FAILURE;
                break;
            }
        }
    }
    running.stop(&print_event).await;
    Ok(code)
}

#[derive(Subcommand)]
enum ComponentCommand {
    /// Download and install a catalog component (resumable, atomic, rolled back on failure).
    Install {
        /// Component id, e.g. `mysql-8-4`.
        id: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        /// Data root (`%LOCALAPPDATA%EchoesOfAzeroth` for the launcher).
        #[arg(long, default_value = ".local/eoa")]
        root: PathBuf,
    },
}

/// Runs one engine operation with console progress.
fn run_operation(layout: &Layout, op: &eoa_engine::Operation<()>) -> Result<()> {
    let engine = eoa_engine::Engine::new(layout.state());
    let runtime = tokio::runtime::Runtime::new()?;
    let started = std::time::Instant::now();
    eprintln!("== {}", op.id());
    let outcome = runtime.block_on(engine.run(
        op,
        &(),
        &ConsoleObserver::default(),
        &eoa_engine::CancelToken::new(),
    ))?;
    eprintln!(
        "== {}: {outcome:?} in {:.1} s",
        op.id(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Prints engine events: steps, and progress at most every 5 %.
#[derive(Default)]
struct ConsoleObserver {
    last_step: std::sync::Mutex<(String, u64)>,
}

impl eoa_engine::Observer for ConsoleObserver {
    fn event(&self, event: &eoa_engine::Event<'_>) {
        use eoa_engine::Event;
        match event {
            Event::StepStarted {
                index,
                total,
                step,
                kind,
                resumed,
            } => {
                let resumed = if *resumed { " (resumed)" } else { "" };
                eprintln!("[{}/{total}] {step} ({kind}){resumed}", index + 1);
            }
            Event::StepSkipped { index, total, step } => {
                eprintln!("[{}/{total}] {step}: already done", index + 1)
            }
            Event::StepProgress {
                step,
                done,
                total: Some(total),
            } => {
                let pct = done * 20 / (*total).max(1);
                let mut last = self.last_step.lock().unwrap();
                if last.0 != *step || last.1 < pct {
                    *last = ((*step).to_owned(), pct);
                    eprintln!(
                        "  {:>3} %  {:.1} / {:.1} MB",
                        pct * 5,
                        *done as f64 / 1e6,
                        *total as f64 / 1e6
                    );
                }
            }
            Event::RollbackStarted { reason } => eprintln!("rolling back: {reason}"),
            Event::StepUndone { step } => eprintln!("  undone {step}"),
            Event::FinalizeFailed { step, error } => {
                eprintln!("  cleanup of {step} failed: {error}")
            }
            _ => {}
        }
    }
}

#[derive(Subcommand)]
enum StoreCommand {
    /// Download a file into the store (parallel, resumable, sha256-verified).
    Fetch {
        /// Store directory.
        #[arg(long, default_value = ".local/store")]
        store: PathBuf,
        /// Source URL (repeat for mirrors, tried in order).
        #[arg(long = "url", required = true)]
        urls: Vec<String>,
        /// Exact size in bytes.
        #[arg(long)]
        size: u64,
        /// Expected sha256 (hex).
        #[arg(long)]
        sha256: String,
    },
}

/// Prints a progress line at most every 5 %.
struct ConsoleProgress {
    total: u64,
    received: std::sync::atomic::AtomicU64,
    last_step: std::sync::atomic::AtomicU64,
}

impl eoa_store::Progress for ConsoleProgress {
    fn advance(&self, bytes: u64) {
        use std::sync::atomic::Ordering::SeqCst;
        let received = self.received.fetch_add(bytes, SeqCst) + bytes;
        let step = received * 20 / self.total.max(1);
        if self.last_step.fetch_max(step, SeqCst) < step {
            eprintln!(
                "  {:>3} %  {:.1} / {:.1} MB",
                step * 5,
                received as f64 / 1e6,
                self.total as f64 / 1e6
            );
        }
    }
    fn retract(&self, bytes: u64) {
        self.received
            .fetch_sub(bytes, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Subcommand)]
enum IndexCommand {
    /// Verify an index (file or URL) with the trusted keys and print it.
    Show {
        /// Defaults to the project's published index.
        source: Option<String>,
    },
}

/// Reads a small document from a file or an `http(s)://` URL.
fn read_document(
    runtime: &tokio::runtime::Runtime,
    store: &eoa_store::Store,
    source: &str,
) -> Result<Vec<u8>> {
    use anyhow::Context;
    let limit = install::index::INDEX_SIZE_LIMIT;
    if source.starts_with("https://") || source.starts_with("http://") {
        Ok(runtime
            .block_on(store.get_document(source, limit))
            .with_context(|| format!("downloading {source}"))?)
    } else {
        Ok(std::fs::read(source).with_context(|| format!("reading {source}"))?)
    }
}

#[derive(Subcommand)]
enum CatalogCommand {
    /// Load the catalog and check references between manifests.
    Validate {
        #[arg(long, default_value = "catalog")]
        path: PathBuf,
    },
    /// Resolve a module selection and print the installation set.
    Resolve {
        #[arg(long, default_value = "catalog")]
        path: PathBuf,
        #[arg(long)]
        emulator: String,
        /// Defaults to the emulator's default flavor.
        #[arg(long)]
        flavor: Option<String>,
        /// Optional module to add (repeatable). Mandatory modules are always added.
        #[arg(long = "module")]
        modules: Vec<String>,
    },
}

fn main() -> Result<ExitCode> {
    match Cli::parse().command {
        Command::Store { command } => match command {
            StoreCommand::Fetch {
                store,
                urls,
                size,
                sha256,
            } => {
                let runtime = tokio::runtime::Runtime::new()?;
                let store = eoa_store::Store::open(store)?;
                let progress = ConsoleProgress {
                    total: size,
                    received: 0.into(),
                    last_step: 0.into(),
                };
                let started = std::time::Instant::now();
                let request = eoa_store::FetchRequest { urls, size, sha256 };
                let path = runtime.block_on(store.fetch(&request, &progress))?;
                let seconds = started.elapsed().as_secs_f64();
                println!(
                    "{} ({:.1} s, {:.1} MB/s)",
                    path.display(),
                    seconds,
                    size as f64 / 1e6 / seconds.max(0.001)
                );
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Component { command } => match command {
            ComponentCommand::Install { id, catalog, root } => {
                let catalog = eoa_catalog::load_dir(&catalog)?;
                let layout = Layout::new(&root);
                let store = Arc::new(eoa_store::Store::open(layout.store())?);
                let op = install::component_operation(&catalog, &layout, store, &id)?;
                run_operation(&layout, &op)?;
                println!("-> {}", layout.component(&id).display());
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Install {
            id,
            emulator,
            flavor,
            modules,
            lan,
            server,
            db_snapshot,
            index,
            catalog,
            root,
        } => {
            let catalog = eoa_catalog::load_dir(&catalog)?;
            let layout = Layout::new(&root);
            let store = Arc::new(eoa_store::Store::open(layout.store())?);
            let emu = &catalog
                .emulator(&emulator)
                .ok_or_else(|| anyhow::anyhow!("unknown emulator '{emulator}'"))?
                .manifest;
            // Shared resources first, each in its own operation.
            for component in &emu.components {
                let op =
                    install::component_operation(&catalog, &layout, store.clone(), &component.id)?;
                run_operation(&layout, &op)?;
            }
            let (op, _) =
                install::server_data_operation(&catalog, &layout, store.clone(), &emulator)?;
            run_operation(&layout, &op)?;

            let request = install::NewInstance {
                id,
                emulator,
                flavor,
                modules,
                network: if lan {
                    install::Network::Lan
                } else {
                    install::Network::Solo
                },
            };
            let (record, secrets) = install::open_or_create(&catalog, &layout, &request)?;
            let (server, db_snapshot) = match server {
                Some(server) => (server, db_snapshot),
                None => {
                    let runtime = tokio::runtime::Runtime::new()?;
                    let source = index.as_deref().unwrap_or(eoa_catalog::index::DEFAULT_URL);
                    let envelope = read_document(&runtime, &store, source)?;
                    let keys = eoa_catalog::index::trusted_keys();
                    let accepted = install::index::accept_index(&layout, &envelope, &keys)?;
                    let build = eoa_catalog::index::select(
                        &accepted,
                        &catalog,
                        &record.emulator,
                        &record.flavor,
                        &install::platform(),
                    )?;
                    let with_snapshot = match &build.db_snapshot {
                        Some(snapshot) => {
                            install::index::snapshot_usable(&catalog, &record, snapshot)?
                        }
                        None => false,
                    };
                    println!(
                        "index serial {}: build {}{}",
                        accepted.serial,
                        build.build_key,
                        if with_snapshot {
                            " with its database snapshot"
                        } else {
                            ", no usable database snapshot (full import)"
                        }
                    );
                    let (op, files) =
                        install::index::build_operation(store.clone(), build, with_snapshot);
                    run_operation(&layout, &op)?;
                    (files.server, files.db_snapshot)
                }
            };
            let op = install::instance_operation(
                &catalog,
                &layout,
                &record,
                &secrets,
                &server,
                db_snapshot.as_deref(),
            )?;
            run_operation(&layout, &op)?;
            println!(
                "instance '{}' ready in {} (database on 127.0.0.1:{})",
                record.id,
                layout.instance(&record.id).root.display(),
                record.db_port
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Start { id, catalog, root } => {
            let catalog = eoa_catalog::load_dir(&catalog)?;
            let server = eoa_runtime::Server::open(&catalog, &Layout::new(&root), &id)?;
            tokio::runtime::Runtime::new()?.block_on(run_instance(server))
        }
        Command::Account {
            command:
                AccountCommand::Create {
                    instance,
                    username,
                    password,
                    gm,
                    catalog,
                    root,
                },
        } => {
            let catalog = eoa_catalog::load_dir(&catalog)?;
            let server = eoa_runtime::Server::open(&catalog, &Layout::new(&root), &instance)?;
            let password = match password {
                Some(password) => password,
                None => {
                    eprint!("password: ");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim_end_matches(['\r', '\n']).to_owned()
                }
            };
            let account = eoa_runtime::NewAccount {
                username,
                password,
                gm_level: gm,
            };
            let driver = driver(server.driver_id())?;
            let id = tokio::runtime::Runtime::new()?
                .block_on(server.create_account(driver.as_ref(), &account))?;
            println!("account '{}' created (id {id})", account.username);
            Ok(ExitCode::SUCCESS)
        }
        Command::Index {
            command: IndexCommand::Show { source },
        } => {
            let runtime = tokio::runtime::Runtime::new()?;
            let store = eoa_store::Store::open(std::env::temp_dir().join("eoa-index-store"))?;
            let source = source.as_deref().unwrap_or(eoa_catalog::index::DEFAULT_URL);
            let envelope = read_document(&runtime, &store, source)?;
            let index = eoa_catalog::index::open(&envelope, &eoa_catalog::index::trusted_keys())?;
            println!("{}", serde_json::to_string_pretty(&index)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Catalog { command } => match command {
            CatalogCommand::Validate { path } => {
                let catalog = eoa_catalog::load_dir(&path)?;
                let issues = eoa_catalog::validate(&catalog);
                for issue in &issues {
                    eprintln!("✗ {issue}");
                }
                if issues.is_empty() {
                    let modules: usize = catalog.emulators.values().map(|e| e.modules.len()).sum();
                    println!(
                        "Catalog OK: {} emulator(s), {} module(s), {} client(s), {} component(s).",
                        catalog.emulators.len(),
                        modules,
                        catalog.clients.len(),
                        catalog.components.len()
                    );
                    Ok(ExitCode::SUCCESS)
                } else {
                    Ok(ExitCode::FAILURE)
                }
            }
            CatalogCommand::Resolve {
                path,
                emulator,
                flavor,
                modules,
            } => {
                let catalog = eoa_catalog::load_dir(&path)?;
                let selected: Vec<&str> = modules.iter().map(String::as_str).collect();
                let resolution =
                    eoa_core::resolve(&catalog, &emulator, flavor.as_deref(), &selected)?;
                println!("emulator: {}", resolution.emulator);
                println!("flavor:   {}", resolution.flavor);
                println!("modules (install order):");
                for module in &resolution.modules {
                    println!("  - {module}");
                }
                for (file, keys) in &resolution.core_overrides {
                    println!("core overrides in {file}:");
                    for (key, (value, owner)) in keys {
                        println!("  {key} = {value}   (imposed by {owner})");
                    }
                }
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

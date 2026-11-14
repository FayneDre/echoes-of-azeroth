//! `eoa`: headless entry point to the same crates the launcher uses.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    /// Shared third-party runtimes (MySQL...).
    Component {
        #[command(subcommand)]
        command: ComponentCommand,
    },
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

/// Catalog platform key of this machine (`windows-x86_64`).
fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
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
                let component = catalog
                    .components
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("unknown component '{id}'"))?;
                let key = platform();
                let build = component
                    .platforms
                    .get(&key)
                    .ok_or_else(|| anyhow::anyhow!("component '{id}' has no build for {key}"))?;
                let store = std::sync::Arc::new(eoa_store::Store::open(root.join("store"))?);
                let request = eoa_store::FetchRequest {
                    urls: vec![build.upstream.clone()],
                    size: build.size,
                    sha256: build.sha256.clone(),
                };
                let blob = store.blob_path(&build.sha256);
                let target = root.join("components").join(&id);
                let op = eoa_engine::Operation::new(format!("component-{id}"))
                    .step(eoa_engine::steps::Fetch::new("download", store, request))
                    .step(
                        eoa_engine::steps::Extract::new("extract", blob, &target)
                            .strip(eoa_engine::steps::Strip::SingleRootDir),
                    );
                let engine = eoa_engine::Engine::new(root.join("state"));
                let runtime = tokio::runtime::Runtime::new()?;
                let started = std::time::Instant::now();
                let outcome = runtime.block_on(engine.run(
                    &op,
                    &(),
                    &ConsoleObserver::default(),
                    &eoa_engine::CancelToken::new(),
                ))?;
                println!(
                    "{id} {}: {outcome:?} in {:.1} s -> {}",
                    component.version,
                    started.elapsed().as_secs_f64(),
                    target.display()
                );
                Ok(ExitCode::SUCCESS)
            }
        },
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

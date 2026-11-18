//! `rk`: headless entry point to the same crates the launcher uses.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rk_engine::install;
use rk_engine::CancelToken;
use rk_launcher::session::{Game, Session, SessionEvent};
use rk_launcher::{InstallRequest, Launcher, UpdateOutcome};

#[derive(Parser)]
#[command(name = "rk", version, about = "RealmKeeper command-line interface")]
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
        /// Data root (`%LOCALAPPDATA%\RealmKeeperData` for the launcher).
        #[arg(long, default_value = ".local/rk")]
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
        /// Data root (`%LOCALAPPDATA%\RealmKeeperData` for the launcher).
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Game accounts of an instance.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Install the server build of the current catalog release on an
    /// instance (core or module pins changed). Databases are backed up first.
    Update {
        id: String,
        /// Server build ZIP; without it, the build is found in the signed index.
        #[arg(long, conflicts_with = "index")]
        server: Option<PathBuf>,
        #[arg(long)]
        index: Option<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Remove an installed realm (servers stopped). Its player data is
    /// backed up first unless --no-backup; backups are never deleted.
    Uninstall {
        id: String,
        /// Skip the last backup of the player data.
        #[arg(long)]
        no_backup: bool,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Reinstall the server files and configs of the current release and
    /// update the databases (backed up first). Player data is kept.
    Repair {
        id: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Resume the interrupted change of an instance (install, update,
    /// repair, module or option change, import).
    Resume {
        id: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Copy, export, import or rename a realm (servers stopped), ADR 0020.
    Realm {
        #[command(subcommand)]
        command: RealmCommand,
    },
    /// Player-data backups of an instance (servers stopped; the app also
    /// backs up a running realm, ADR 0022).
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
    /// Turn modules on or off on an installed instance (servers stopped).
    Module {
        #[command(subcommand)]
        command: ModuleCommand,
    },
    /// Module options of an instance (servers stopped).
    Option {
        #[command(subcommand)]
        command: OptionCommand,
    },
    /// Your game client (never distributed: point the launcher at yours).
    Client {
        #[command(subcommand)]
        command: ClientCommand,
    },
    /// Prepare the client, start the servers and launch the game once the
    /// world is ready. The servers stop when the game is closed.
    Play {
        id: String,
        /// Keep the servers running after the game is closed.
        #[arg(long)]
        keep_running: bool,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Print a support report (versions, realms, logs; personal folders
    /// redacted) to paste into a bug report.
    Diagnostic {
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Windows firewall rules that let the other computers of the network
    /// reach a realm's servers (ADR 0022; allow and remove ask for elevation).
    Firewall {
        #[command(subcommand)]
        command: FirewallCommand,
    },
    /// Join a realm hosted on another computer of the local network: point
    /// the game at it and launch it (no server runs here).
    Join {
        address: String,
        #[arg(long, default_value = "azerothcore")]
        emulator: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum FirewallCommand {
    /// Whether the rules are there.
    Show {
        instance: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Make the rules (one per server program, private networks).
    Allow {
        instance: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Remove the rules.
    Remove {
        instance: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum OptionCommand {
    /// Set option values (`<option>=<value>`); the default value clears it.
    Set {
        instance: String,
        module: String,
        #[arg(required = true, value_name = "OPTION=VALUE")]
        values: Vec<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Show the options of the instance's modules and their values.
    Show {
        instance: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum ClientCommand {
    /// Register the game folder (the one that contains the executable).
    Set {
        path: PathBuf,
        /// Client manifest; defaults to the only one in the catalog.
        #[arg(long)]
        client: Option<String>,
        /// Game language (e.g. enUS, frFR) when several are installed.
        #[arg(long)]
        locale: Option<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Show the registered clients and the files the launcher put in them.
    Show {
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Look for the game in the usual places (drives, personal folders).
    Find {
        /// Client manifest; defaults to the only one in the catalog.
        #[arg(long)]
        client: Option<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Prepare the client for an instance without starting anything
    /// (realmlist, cache, the instance's patch files); `rk play` does it too.
    Prepare {
        instance: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum RealmCommand {
    /// Copy a stopped realm to a new one (same build, modules, characters).
    Duplicate {
        source: String,
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Write a stopped realm (choices, accounts, characters) to one file.
    Export {
        id: String,
        file: PathBuf,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Install a new realm from an exported file and put its player data back.
    Import {
        file: PathBuf,
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        index: Option<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Give a realm a name (none to remove it).
    Rename {
        id: String,
        name: Option<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum BackupCommand {
    /// List the backups, newest first.
    List {
        instance: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Dump the player databases to a new manual backup.
    Create {
        instance: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Put the player databases back from a backup (the current state is
    /// backed up first).
    Restore {
        instance: String,
        name: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Delete a backup file.
    Delete {
        instance: String,
        name: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum ModuleCommand {
    /// Enable modules: config, SQL and post-install steps (accounts...).
    Enable {
        instance: String,
        #[arg(required = true)]
        modules: Vec<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
    /// Disable modules (their config makes them inert; data is kept).
    Disable {
        instance: String,
        #[arg(required = true)]
        modules: Vec<String>,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

fn open(catalog: &Path, root: &Path) -> Result<Launcher> {
    Ok(Launcher::open(catalog, root)?)
}

fn block_on<F: std::future::Future>(future: F) -> Result<F::Output> {
    Ok(tokio::runtime::Runtime::new()?.block_on(future))
}

/// `rk module enable|disable`.
fn change_modules(
    catalog: &Path,
    root: &Path,
    instance: &str,
    enable: &[String],
    disable: &[String],
) -> Result<ExitCode> {
    let launcher = open(catalog, root)?;
    let rebuilds = launcher.rebuilds_for(instance, disable)?;
    if !rebuilds.is_empty() {
        eprintln!(
            "rebuilding {} without the removed module(s): this takes several minutes",
            rebuilds.join(", ")
        );
    }
    let observer = ConsoleObserver::default();
    let cancel = CancelToken::new();
    let record = block_on(launcher.set_modules(instance, enable, disable, &observer, &cancel))??;
    println!(
        "instance '{}' modules: {}",
        record.id,
        record.modules.join(", ")
    );
    Ok(ExitCode::SUCCESS)
}

/// `rk option set`: saves the values, then applies them like a module change.
fn set_options(
    catalog: &Path,
    root: &Path,
    instance: &str,
    module: &str,
    values: &[String],
) -> Result<ExitCode> {
    let launcher = open(catalog, root)?;
    let pairs = values
        .iter()
        .map(|v| {
            v.split_once('=')
                .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                .ok_or_else(|| anyhow::anyhow!("'{v}': expected <option>=<value>"))
        })
        .collect::<Result<Vec<_>>>()?;
    let observer = ConsoleObserver::default();
    let cancel = CancelToken::new();
    let record = block_on(launcher.set_options(instance, module, &pairs, &observer, &cancel))??;
    print_options(&launcher, &record);
    Ok(ExitCode::SUCCESS)
}

fn print_options(launcher: &Launcher, record: &install::InstanceRecord) {
    // Realm settings first (`rk option set <id> realm <option>=<value>`).
    let realm: Vec<_> = launcher
        .realm_settings(record)
        .into_iter()
        .flat_map(|s| s.options)
        .collect();
    let modules = launcher
        .options(record)
        .into_iter()
        .map(|m| (m.module, m.options));
    let all = (!realm.is_empty())
        .then(|| (rk_core::manifest::REALM_OPTIONS.to_owned(), realm))
        .into_iter()
        .chain(modules);
    for (owner, views) in all {
        println!("{owner}:");
        for view in views {
            print_option(&view);
        }
    }
}

fn print_option(view: &rk_launcher::OptionView) {
    use rk_core::manifest::OptionValue;
    let changed = view.value != view.option.default_value();
    let text = match &view.value {
        OptionValue::Bool(b) => b.to_string(),
        OptionValue::Integer(n) => n.to_string(),
        OptionValue::Number(n) => n.to_string(),
        OptionValue::Choice(c) => c.clone(),
    };
    let mark = if changed { "  (changed)" } else { "" };
    println!("  {} = {text}{mark}", view.option.id());
}

/// The client manifest named, or the only one in the catalog.
fn client_id(launcher: &Launcher, id: Option<&str>) -> Result<String> {
    match id {
        Some(id) => Ok(id.to_owned()),
        None => {
            let mut all = launcher.catalog().clients.keys();
            match (all.next(), all.next()) {
                (Some(one), None) => Ok(one.clone()),
                _ => anyhow::bail!("several clients in the catalog: choose one with --client"),
            }
        }
    }
}

/// Points the registered client at the instance and prints what changed.
fn prepare_client(
    launcher: &Launcher,
    record: &install::InstanceRecord,
) -> Result<rk_launcher::PreparedClient> {
    let prepared = launcher.prepare_client(record)?;
    let report = &prepared.report;
    eprintln!(
        "== client {} prepared for '{}' ({})",
        prepared.state.path.display(),
        record.id,
        report.realmlist.display()
    );
    if report.cache_cleared {
        eprintln!("   cache cleared (another server was played last)");
    }
    for added in &report.added {
        eprintln!("   + {added}");
    }
    for removed in &report.removed {
        eprintln!("   - {removed}");
    }
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(prepared)
}

/// `rk play`: client prepared first (fails fast), then the servers, then
/// the game once the world is ready.
fn play(catalog: &Path, root: &Path, id: &str, keep_running: bool) -> Result<ExitCode> {
    let launcher = open(catalog, root)?;
    let server = launcher.server(id)?;
    let client = prepare_client(&launcher, server.record())?;
    let game = Game {
        client,
        keep_running,
    };
    block_on(run_instance(server, Some(game)))?
}

fn client_command(command: ClientCommand) -> Result<ExitCode> {
    match command {
        ClientCommand::Set {
            path,
            client,
            locale,
            catalog,
            root,
        } => {
            let launcher = open(&catalog, &root)?;
            let client = client_id(&launcher, client.as_deref())?;
            let (state, warning) = launcher.register_client(&client, &path, locale.as_deref())?;
            if let Some(warning) = warning {
                eprintln!("warning: {warning}");
            }
            let manifest = &launcher.catalog().clients[&client];
            println!(
                "{} client registered: {} ({}, build {})",
                manifest.version,
                state.path.display(),
                state.locale,
                manifest.detection.build
            );
            Ok(ExitCode::SUCCESS)
        }
        ClientCommand::Prepare {
            instance,
            catalog,
            root,
        } => {
            let launcher = open(&catalog, &root)?;
            let record = launcher.instance(&instance)?.record;
            prepare_client(&launcher, &record)?;
            Ok(ExitCode::SUCCESS)
        }
        ClientCommand::Find {
            client,
            catalog,
            root,
        } => {
            let launcher = open(&catalog, &root)?;
            let client = client_id(&launcher, client.as_deref())?;
            let found = rk_client::find(&launcher.catalog().clients[&client]);
            if found.is_empty() {
                println!("no {client} client found");
            }
            for client in found {
                println!("{} ({})", client.root.display(), client.locales.join(", "));
            }
            Ok(ExitCode::SUCCESS)
        }
        ClientCommand::Show { catalog, root } => {
            let launcher = open(&catalog, &root)?;
            for manifest in launcher.catalog().clients.values() {
                match launcher.client(&manifest.id)? {
                    None => println!("{}: not registered", manifest.id),
                    Some(state) => {
                        let status = match rk_client::detect(manifest, &state.path) {
                            Ok(_) => "ok".to_owned(),
                            Err(e) => e.to_string(),
                        };
                        println!(
                            "{}: {} ({}) - {status}",
                            manifest.id,
                            state.path.display(),
                            state.locale
                        );
                        for file in &state.managed {
                            println!("  managed: {}", file.path);
                        }
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
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
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

/// `rk update`: installs the server build of the current catalog release.
fn update_instance(
    catalog: &Path,
    root: &Path,
    id: &str,
    server: Option<PathBuf>,
    index: Option<&str>,
) -> Result<ExitCode> {
    let launcher = open(catalog, root)?;
    let observer = ConsoleObserver::default();
    let cancel = CancelToken::new();
    eprintln!("updating '{id}'; every database is backed up first");
    match block_on(launcher.update(id, server.as_deref(), index, &observer, &cancel))?? {
        UpdateOutcome::UpToDate { build } => println!("instance '{id}' already runs build {build}"),
        UpdateOutcome::Updated { from, to } => println!(
            "instance '{id}' updated from build {} to {to}",
            from.as_deref().unwrap_or("(unknown)")
        ),
    }
    Ok(ExitCode::SUCCESS)
}

/// Prints session progress and server output.
fn print_event(event: SessionEvent) {
    use SessionEvent as E;
    match event {
        E::DatabaseStarting => eprintln!("== starting the database"),
        E::DatabaseReady => eprintln!("== database ready"),
        E::ProcessStarting { process } => eprintln!("== starting {process}"),
        E::ProcessReady { process, after_ms } => {
            eprintln!("== {process} ready after {:.1} s", after_ms as f64 / 1000.0)
        }
        E::Ready { .. } => {}
        E::ProcessStopping { process } => eprintln!("== stopping {process}"),
        E::ProcessStopped { process, how } => eprintln!("== {process} stopped ({how:?})"),
        E::DatabaseStopped => eprintln!("== database stopped"),
        E::Warning { message } => eprintln!("warning: {message}"),
        E::Log { process, text, .. } => println!("[{process}] {text}"),
        E::LogSkipped { lines } => eprintln!("({lines} lines skipped)"),
        E::GameStarted { .. } => eprintln!("== game started"),
        E::GameExited { code } => eprintln!("== the game was closed (code {code:?})"),
        E::ProcessExited { process, code } => {
            eprintln!("== {process} exited (code {code:?}); stopping the others")
        }
        // Shown by the app; the console output already says who logs in.
        E::Population { .. } => {}
        E::Startup {
            process,
            progress,
            label,
        } => eprintln!(
            "== {process}: {} ({:.0} %)",
            label.en,
            f64::from(progress) * 100.0
        ),
        E::Failed { error } => eprintln!("error: {}", error.detail),
        E::Ended { .. } => {}
    }
}

/// `rk start` / `rk play`: runs until Ctrl+C, `.stop`, end of input, a
/// server exit, or (with a game, unless kept running) the game's exit.
async fn run_instance(server: rk_runtime::Server, game: Option<Game>) -> Result<ExitCode> {
    let network = server.record().network;
    let with_game = game.is_some();
    let keep_running = game.as_ref().is_none_or(|g| g.keep_running);
    let events: rk_launcher::session::EventSink = Arc::new(move |event| {
        if let SessionEvent::Ready { address, auth_port } = &event {
            match (with_game, network) {
                (false, _) => eprintln!(
                    "== ready: set your client's realmlist to {address} (auth port {auth_port}). \
                     Type console commands; Ctrl+C or .stop to stop."
                ),
                (true, install::Network::Lan) => eprintln!(
                    "== ready: other computers use realmlist {address} (auth port {auth_port}). \
                     Type console commands; the servers stop when the game is closed."
                ),
                (true, install::Network::Solo) => eprintln!(
                    "== ready. Type console commands; {}",
                    if keep_running {
                        "Ctrl+C or .stop to stop the servers."
                    } else {
                        "the servers stop when the game is closed."
                    }
                ),
            }
        }
        print_event(event);
    });
    let session = Session::start(server, game, events)?;
    let control = session.control();

    // stdin is read on a plain thread: tokio's stdin cannot be interrupted.
    let input = control.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if line.trim() == ".stop" {
                break;
            }
            input.console(&line);
        }
        input.stop();
    });
    let ctrl_c = control.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            ctrl_c.stop();
        }
    });
    Ok(if session.wait().await {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[derive(Subcommand)]
enum ComponentCommand {
    /// Download and install a catalog component (resumable, atomic, rolled back on failure).
    Install {
        /// Component id, e.g. `mysql-8-4`.
        id: String,
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,
        /// Data root (`%LOCALAPPDATA%\RealmKeeperData` for the launcher).
        #[arg(long, default_value = ".local/rk")]
        root: PathBuf,
    },
}

/// Prints engine events: steps, and progress at most every 5 %.
#[derive(Default)]
struct ConsoleObserver {
    last_step: std::sync::Mutex<(String, u64)>,
}

impl rk_engine::Observer for ConsoleObserver {
    fn event(&self, event: &rk_engine::Event<'_>) {
        use rk_engine::Event;
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

impl rk_store::Progress for ConsoleProgress {
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
    store: &rk_store::Store,
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
                let store = rk_store::Store::open(store)?;
                let progress = ConsoleProgress {
                    total: size,
                    received: 0.into(),
                    last_step: 0.into(),
                };
                let started = std::time::Instant::now();
                let request = rk_store::FetchRequest { urls, size, sha256 };
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
                let launcher = open(&catalog, &root)?;
                let op = install::component_operation(
                    launcher.catalog(),
                    launcher.layout(),
                    launcher.store().clone(),
                    &id,
                )?;
                let observer = ConsoleObserver::default();
                block_on(launcher.run(&op, &observer, &CancelToken::new()))??;
                println!("-> {}", launcher.layout().component(&id).display());
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
            let launcher = open(&catalog, &root)?;
            let request = InstallRequest {
                id,
                emulator,
                flavor,
                modules,
                network: if lan {
                    install::Network::Lan
                } else {
                    install::Network::Solo
                },
                server,
                db_snapshot,
                index,
            };
            let observer = ConsoleObserver::default();
            let record = block_on(launcher.install(&request, &observer, &CancelToken::new()))??;
            println!(
                "instance '{}' ready in {} (database on 127.0.0.1:{})",
                record.id,
                launcher.layout().instance(&record.id).root.display(),
                record.db_port
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Start { id, catalog, root } => {
            let launcher = open(&catalog, &root)?;
            let server = launcher.server(&id)?;
            block_on(run_instance(server, None))?
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
            let launcher = open(&catalog, &root)?;
            let password = match password {
                Some(password) => password,
                None => {
                    eprint!("password: ");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim_end_matches(['\r', '\n']).to_owned()
                }
            };
            let account = rk_runtime::NewAccount {
                username,
                password,
                gm_level: gm,
            };
            let id = block_on(launcher.create_account(&instance, &account))??;
            println!("account '{}' created (id {id})", account.username);
            Ok(ExitCode::SUCCESS)
        }
        Command::Update {
            id,
            server,
            index,
            catalog,
            root,
        } => update_instance(&catalog, &root, &id, server, index.as_deref()),
        Command::Uninstall {
            id,
            no_backup,
            catalog,
            root,
        } => {
            let launcher = open(&catalog, &root)?;
            let report = block_on(launcher.uninstall(&id, !no_backup))??;
            if let Some(backup) = &report.backup {
                println!(
                    "player data saved to {}",
                    launcher.backups_dir(&id).join(&backup.name).display()
                );
            }
            for warning in &report.warnings {
                eprintln!("warning: {warning}");
            }
            println!("instance '{id}' removed");
            if report.shared_removed {
                println!("no realm left: shared downloads removed");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Repair { id, catalog, root } => {
            let launcher = open(&catalog, &root)?;
            let observer = ConsoleObserver::default();
            eprintln!("repairing '{id}'; every database is backed up first");
            let record = block_on(launcher.repair(&id, &observer, &CancelToken::new()))??;
            println!(
                "instance '{id}' repaired (build {})",
                record.build.as_deref().unwrap_or("unknown")
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Resume { id, catalog, root } => {
            let launcher = open(&catalog, &root)?;
            let observer = ConsoleObserver::default();
            let summary = launcher.instance(&id)?;
            match summary.unfinished {
                None if summary.moved => {
                    eprintln!("the realm folder moved: writing its paths again");
                    block_on(launcher.resume(&id, &observer, &CancelToken::new()))??;
                    println!("instance '{id}': paths updated");
                }
                None => println!("instance '{id}' has no interrupted change"),
                Some(op) => {
                    eprintln!("resuming {op}");
                    block_on(launcher.resume(&id, &observer, &CancelToken::new()))??;
                    println!("instance '{id}': {op} finished");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Diagnostic { catalog, root } => {
            let launcher = open(&catalog, &root)?;
            print!(
                "{}",
                rk_launcher::diagnostic::report(&launcher, env!("CARGO_PKG_VERSION"), &[])
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Firewall { command } => {
            let (instance, catalog, root, action) = match command {
                FirewallCommand::Show {
                    instance,
                    catalog,
                    root,
                } => (instance, catalog, root, "show"),
                FirewallCommand::Allow {
                    instance,
                    catalog,
                    root,
                } => (instance, catalog, root, "allow"),
                FirewallCommand::Remove {
                    instance,
                    catalog,
                    root,
                } => (instance, catalog, root, "remove"),
            };
            let launcher = open(&catalog, &root)?;
            let status = block_on(async {
                match action {
                    "allow" => launcher.allow_firewall(&instance).await,
                    "remove" => launcher.remove_firewall(&instance).await,
                    _ => launcher.firewall_status(&instance).await,
                }
            })??;
            if !status.supported {
                println!("firewall rules are only made on Windows");
            } else {
                println!(
                    "{instance}: {}",
                    if status.allowed {
                        "other computers of the local network can connect"
                    } else {
                        "no firewall rule: other computers may be blocked"
                    }
                );
                if status.public_network {
                    println!("Windows treats this network as public: the rules apply to private networks only");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Join {
            address,
            emulator,
            catalog,
            root,
        } => {
            let launcher = open(&catalog, &root)?;
            let client = launcher.prepare_client_for_lan(&emulator, &address)?;
            eprintln!("realmlist set to {address}; launching the game");
            block_on(async {
                let mut child = rk_client::launch(&client.manifest, &client.state)?;
                child.wait().await.map_err(anyhow::Error::from)?;
                anyhow::Ok(())
            })??;
            drop(client);
            Ok(ExitCode::SUCCESS)
        }
        Command::Realm { command } => match command {
            RealmCommand::Duplicate {
                source,
                id,
                name,
                catalog,
                root,
            } => {
                let launcher = open(&catalog, &root)?;
                let observer = ConsoleObserver::default();
                eprintln!("copying realm '{source}' to '{id}'");
                let record = block_on(launcher.duplicate(
                    &source,
                    &id,
                    name.as_deref(),
                    &observer,
                    &CancelToken::new(),
                ))??;
                println!(
                    "realm '{}' created (database port {})",
                    record.id, record.db_port
                );
                Ok(ExitCode::SUCCESS)
            }
            RealmCommand::Export {
                id,
                file,
                catalog,
                root,
            } => {
                let launcher = open(&catalog, &root)?;
                eprintln!("exporting realm '{id}'");
                let summary = block_on(launcher.export(&id, &file))??;
                println!(
                    "realm '{id}' exported to {} (player data {:.1} MB)",
                    file.display(),
                    summary.dump_size as f64 / 1e6
                );
                Ok(ExitCode::SUCCESS)
            }
            RealmCommand::Import {
                file,
                id,
                name,
                index,
                catalog,
                root,
            } => {
                let launcher = open(&catalog, &root)?;
                let summary = launcher.read_export(&file)?;
                eprintln!(
                    "importing {} {} with {} (player data {:.1} MB) as '{id}'",
                    summary.realm.emulator,
                    summary.realm.flavor,
                    summary.realm.modules.join(", "),
                    summary.dump_size as f64 / 1e6
                );
                let observer = ConsoleObserver::default();
                let record = block_on(launcher.import(
                    &file,
                    &id,
                    name.as_deref(),
                    index.as_deref(),
                    &observer,
                    &CancelToken::new(),
                ))??;
                println!("realm '{}' imported", record.id);
                Ok(ExitCode::SUCCESS)
            }
            RealmCommand::Rename {
                id,
                name,
                catalog,
                root,
            } => {
                let record = open(&catalog, &root)?.rename(&id, name.as_deref())?;
                match record.name {
                    Some(name) => println!("realm '{id}' is now named {name}"),
                    None => println!("realm '{id}' has no name"),
                }
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Backup { command } => match command {
            BackupCommand::List {
                instance,
                catalog,
                root,
            } => {
                let launcher = open(&catalog, &root)?;
                for b in launcher.backups(&instance)? {
                    println!(
                        "{:<56} {:>8.1} MB  {:?}{}",
                        b.name,
                        b.size as f64 / 1e6,
                        b.kind,
                        if b.automatic { "" } else { " (kept)" }
                    );
                }
                Ok(ExitCode::SUCCESS)
            }
            BackupCommand::Create {
                instance,
                catalog,
                root,
            } => {
                let launcher = open(&catalog, &root)?;
                let backup = block_on(launcher.backup_now(&instance))??;
                println!(
                    "backup {} ({:.1} MB)",
                    backup.name,
                    backup.size as f64 / 1e6
                );
                Ok(ExitCode::SUCCESS)
            }
            BackupCommand::Restore {
                instance,
                name,
                catalog,
                root,
            } => {
                let launcher = open(&catalog, &root)?;
                let observer = ConsoleObserver::default();
                eprintln!("restoring the player databases of '{instance}' from {name}");
                block_on(launcher.restore(&instance, &name, &observer, &CancelToken::new()))??;
                println!("instance '{instance}' restored from {name}");
                Ok(ExitCode::SUCCESS)
            }
            BackupCommand::Delete {
                instance,
                name,
                catalog,
                root,
            } => {
                open(&catalog, &root)?.delete_backup(&instance, &name)?;
                println!("deleted {name}");
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Module { command } => match command {
            ModuleCommand::Enable {
                instance,
                modules,
                catalog,
                root,
            } => change_modules(&catalog, &root, &instance, &modules, &[]),
            ModuleCommand::Disable {
                instance,
                modules,
                catalog,
                root,
            } => change_modules(&catalog, &root, &instance, &[], &modules),
        },
        Command::Option { command } => match command {
            OptionCommand::Set {
                instance,
                module,
                values,
                catalog,
                root,
            } => set_options(&catalog, &root, &instance, &module, &values),
            OptionCommand::Show {
                instance,
                catalog,
                root,
            } => {
                let launcher = open(&catalog, &root)?;
                let record = launcher.instance(&instance)?.record;
                print_options(&launcher, &record);
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Client { command } => client_command(command),
        Command::Play {
            id,
            keep_running,
            catalog,
            root,
        } => play(&catalog, &root, &id, keep_running),
        Command::Index {
            command: IndexCommand::Show { source },
        } => {
            let runtime = tokio::runtime::Runtime::new()?;
            let store = rk_store::Store::open(std::env::temp_dir().join("rk-index-store"))?;
            let source = source.as_deref().unwrap_or(rk_catalog::index::DEFAULT_URL);
            let envelope = read_document(&runtime, &store, source)?;
            let index = rk_catalog::index::open(&envelope, &rk_catalog::index::trusted_keys())?;
            println!("{}", serde_json::to_string_pretty(&index)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Catalog { command } => match command {
            CatalogCommand::Validate { path } => {
                let catalog = rk_catalog::load_dir(&path)?;
                let issues = rk_catalog::validate(&catalog);
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
                let catalog = rk_catalog::load_dir(&path)?;
                let selected: Vec<&str> = modules.iter().map(String::as_str).collect();
                let resolution =
                    rk_core::resolve(&catalog, &emulator, flavor.as_deref(), &selected)?;
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

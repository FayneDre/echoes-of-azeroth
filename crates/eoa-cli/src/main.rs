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

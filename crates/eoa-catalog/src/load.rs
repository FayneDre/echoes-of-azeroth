use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eoa_core::catalog::{Catalog, EmulatorEntry};
use eoa_core::SCHEMA_VERSION;
use serde::de::DeserializeOwned;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid manifest {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error(
        "{path}: unsupported schemaVersion {found} (this launcher supports {})",
        SCHEMA_VERSION
    )]
    SchemaVersion { path: PathBuf, found: u32 },
    #[error("{path}: id '{id}' must match its location '{expected}'")]
    IdMismatch {
        path: PathBuf,
        id: String,
        expected: String,
    },
}

/// Loads a catalog from a directory with the standard layout.
pub fn load_dir(root: &Path) -> Result<Catalog, LoadError> {
    let mut catalog = Catalog::default();

    for path in json_files(&root.join("clients"))? {
        let m: eoa_core::manifest::ClientManifest = read(&path)?;
        check(&path, m.schema_version, &m.id, &stem(&path))?;
        catalog.clients.insert(m.id.clone(), m);
    }
    for path in json_files(&root.join("components"))? {
        let m: eoa_core::manifest::ComponentManifest = read(&path)?;
        check(&path, m.schema_version, &m.id, &stem(&path))?;
        catalog.components.insert(m.id.clone(), m);
    }

    for dir in subdirs(&root.join("emulators"))? {
        let dir_name = stem(&dir);
        let path = dir.join("emulator.json");
        let manifest: eoa_core::manifest::EmulatorManifest = read(&path)?;
        check(&path, manifest.schema_version, &manifest.id, &dir_name)?;

        let mut flavors = BTreeMap::new();
        for path in json_files(&dir.join("flavors"))? {
            let m: eoa_core::manifest::FlavorManifest = read(&path)?;
            check(&path, m.schema_version, &m.id, &stem(&path))?;
            check(&path, m.schema_version, &m.emulator, &dir_name)?;
            flavors.insert(m.id.clone(), m);
        }
        let mut modules = BTreeMap::new();
        for path in json_files(&dir.join("modules"))? {
            let m: eoa_core::manifest::ModuleManifest = read(&path)?;
            check(&path, m.schema_version, &m.id, &stem(&path))?;
            check(&path, m.schema_version, &m.emulator, &dir_name)?;
            modules.insert(m.id.clone(), m);
        }
        catalog.emulators.insert(
            manifest.id.clone(),
            EmulatorEntry {
                manifest,
                flavors,
                modules,
            },
        );
    }

    Ok(catalog)
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T, LoadError> {
    let text = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.into(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| LoadError::Parse {
        path: path.into(),
        source,
    })
}

fn check(path: &Path, version: u32, id: &str, expected: &str) -> Result<(), LoadError> {
    if version != SCHEMA_VERSION {
        return Err(LoadError::SchemaVersion {
            path: path.into(),
            found: version,
        });
    }
    if id != expected {
        return Err(LoadError::IdMismatch {
            path: path.into(),
            id: id.into(),
            expected: expected.into(),
        });
    }
    Ok(())
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Sorted for deterministic loading; a missing directory is simply empty.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let io = |source| LoadError::Io {
        path: dir.into(),
        source,
    };
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        paths.push(entry.map_err(io)?.path());
    }
    paths.sort();
    Ok(paths)
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
    Ok(entries(dir)?
        .into_iter()
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
        .collect())
}

fn subdirs(dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
    Ok(entries(dir)?.into_iter().filter(|p| p.is_dir()).collect())
}

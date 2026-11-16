//! The player's own game client (ADR 0013, principle 7: never distributed).
//!
//! - [`detect`]: the executable's version resource must carry the build the
//!   catalog expects; the locale comes from the `Data/<locale>` directories.
//! - [`register`] / [`load`]: one registered client per client manifest,
//!   in `clients/<client id>.json` under the launcher root.
//! - [`prepare`], before every launch: realmlist (and the address the
//!   client saved in its settings), cache cleared when the server changes,
//!   managed patch files reconciled with the instance's choices.
//! - [`launch`]: starts the executable from the client directory.
//!
//! Managed files are tracked with their sha256. A file the player had put
//! at the same place is moved aside (`<name>.rk-backup`) and put back when
//! the launcher removes its own; a tracked file the player changed is never
//! deleted. Each change is recorded before it is made, so an interrupted
//! preparation is finished by the next one.

#![forbid(unsafe_code)]

mod files;
mod find;
mod version;

use std::path::{Path, PathBuf};

use rk_core::manifest::ClientManifest;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use files::ManagedFile;
pub use find::find;
pub use version::executable_build;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("no game client in {0}")]
    NotFound(PathBuf),
    #[error("{0} is missing: choose the folder that contains it")]
    MissingExecutable(PathBuf),
    #[error("{path} is build {found}, this server needs build {expected} ({version})")]
    WrongBuild {
        path: PathBuf,
        found: u32,
        expected: u32,
        version: String,
    },
    #[error("cannot read the version of {0}")]
    UnreadableVersion(PathBuf),
    #[error("no supported language folder in {0}")]
    NoLocale(PathBuf),
    #[error("several languages are installed ({0}): choose one")]
    AmbiguousLocale(String),
    #[error("language '{locale}' is not installed in this client (found: {found})")]
    UnknownLocale { locale: String, found: String },
    #[error("cannot write in {0}: the folder is read-only or protected (move the game out of Program Files)")]
    NotWritable(PathBuf),
    #[error("{path} already exists and is not the launcher's: move it away first")]
    Conflict { path: PathBuf },
    #[error("no game client registered for {0}")]
    NotRegistered(String),
    #[error("the {0} client is in use by another launcher command (a game is running)")]
    Busy(String),
    #[error("{0} is in use: close the game first")]
    InUse(PathBuf),
    #[error("{path}: {message}")]
    State { path: PathBuf, message: String },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl ClientError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ClientError + '_ {
        move |source| {
            // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION: the game has
            // the file open.
            if cfg!(windows) && matches!(source.raw_os_error(), Some(32 | 33)) {
                ClientError::InUse(path.to_path_buf())
            } else if source.kind() == std::io::ErrorKind::PermissionDenied {
                ClientError::NotWritable(path.to_path_buf())
            } else {
                ClientError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        }
    }
}

/// What [`detect`] found in a client directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub root: PathBuf,
    pub build: u32,
    /// Locales of the manifest that have a `Data/<locale>` directory.
    pub locales: Vec<String>,
}

pub fn detect(manifest: &ClientManifest, root: &Path) -> Result<Detected, ClientError> {
    if !root.is_dir() {
        return Err(ClientError::NotFound(root.to_path_buf()));
    }
    let exe = root.join(&manifest.detection.executable);
    if !exe.is_file() {
        return Err(ClientError::MissingExecutable(exe));
    }
    let build =
        executable_build(&exe)?.ok_or_else(|| ClientError::UnreadableVersion(exe.clone()))?;
    if build != manifest.detection.build {
        return Err(ClientError::WrongBuild {
            path: exe,
            found: build,
            expected: manifest.detection.build,
            version: manifest.version.clone(),
        });
    }
    let data = root.join(&manifest.patches.directory);
    let locales = manifest
        .locales
        .iter()
        .filter(|l| data.join(l).is_dir())
        .cloned()
        .collect();
    Ok(Detected {
        root: root.to_path_buf(),
        build,
        locales,
    })
}

/// A registered client and what the launcher changed in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientState {
    pub format: u32,
    /// Client manifest id.
    pub client: String,
    pub path: PathBuf,
    pub locale: String,
    /// Files the launcher put in the client.
    #[serde(default)]
    pub managed: Vec<ManagedFile>,
    /// Instance the client was last prepared for (its cache belongs to it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_server: Option<String>,
}

fn state_path(dir: &Path, client: &str) -> PathBuf {
    dir.join(format!("{client}.json"))
}

/// Exclusive OS lock on one registered client, released when dropped (also
/// on crash): held while it is prepared and while the game started by the
/// launcher runs, so two sessions never rewrite its files at once.
#[derive(Debug)]
pub struct ClientLock {
    _file: std::fs::File,
}

pub fn lock(dir: &Path, client: &str) -> Result<ClientLock, ClientError> {
    std::fs::create_dir_all(dir).map_err(ClientError::io(dir))?;
    let path = dir.join(format!("{client}.lock"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(ClientError::io(&path))?;
    match file.try_lock() {
        Ok(()) => Ok(ClientLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(ClientError::Busy(client.to_owned())),
        Err(std::fs::TryLockError::Error(e)) => Err(ClientError::io(&path)(e)),
    }
}

pub fn load(dir: &Path, client: &str) -> Result<Option<ClientState>, ClientError> {
    let path = state_path(dir, client);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| ClientError::State {
                path,
                message: e.to_string(),
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ClientError::io(&path)(e)),
    }
}

pub(crate) fn save(dir: &Path, state: &ClientState) -> Result<(), ClientError> {
    let path = state_path(dir, &state.client);
    let json = serde_json::to_vec_pretty(state).expect("state serializes");
    files::write_atomic(&path, &json)
}

/// Detects the client at `root` and registers it for its manifest. The
/// locale is required when several are installed. Re-registering the same
/// folder keeps the record of managed files; a new folder starts clean
/// (the old one keeps its files, listed in the returned warning).
pub fn register(
    dir: &Path,
    manifest: &ClientManifest,
    root: &Path,
    locale: Option<&str>,
) -> Result<(ClientState, Option<String>), ClientError> {
    let root = std::path::absolute(root).map_err(ClientError::io(root))?;
    let detected = detect(manifest, &root)?;
    let found = detected.locales.join(", ");
    let locale = match locale {
        Some(l) if detected.locales.iter().any(|x| x == l) => l.to_owned(),
        Some(l) => {
            return Err(ClientError::UnknownLocale {
                locale: l.to_owned(),
                found,
            })
        }
        None => match detected.locales.as_slice() {
            [one] => one.clone(),
            [] => return Err(ClientError::NoLocale(root)),
            _ => return Err(ClientError::AmbiguousLocale(found)),
        },
    };
    let previous = load(dir, &manifest.id)?;
    let mut warning = None;
    let managed = match previous {
        Some(p) if p.path == root => p.managed,
        Some(p) if !p.managed.is_empty() => {
            warning = Some(format!(
                "the previous client {} keeps the launcher's files: {}",
                p.path.display(),
                p.managed
                    .iter()
                    .map(|m| m.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            Vec::new()
        }
        _ => Vec::new(),
    };
    let state = ClientState {
        format: 1,
        client: manifest.id.clone(),
        path: root,
        locale,
        managed,
        last_server: None,
    };
    std::fs::create_dir_all(dir).map_err(ClientError::io(dir))?;
    save(dir, &state)?;
    Ok((state, warning))
}

/// What [`prepare`] did, for the player.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PrepareReport {
    pub realmlist: PathBuf,
    pub cache_cleared: bool,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub warnings: Vec<String>,
}

/// Points the client at `address` and reconciles its managed files with
/// `files` (source file, destination relative to the patch directory) and
/// `addons` (relative to the add-on directory). `server` identifies the
/// instance (cache ownership).
pub fn prepare(
    dir: &Path,
    manifest: &ClientManifest,
    state: &mut ClientState,
    address: &str,
    server: &str,
    files: &[(PathBuf, String)],
    addons: &[(PathBuf, String)],
) -> Result<PrepareReport, ClientError> {
    let detected = detect(manifest, &state.path)?;
    if !detected.locales.contains(&state.locale) {
        // Never create a language folder: the client was changed.
        return Err(ClientError::UnknownLocale {
            locale: state.locale.clone(),
            found: detected.locales.join(", "),
        });
    }
    let mut report = PrepareReport::default();
    let root = state.path.clone();

    // Realmlist, and the address the client saved in its settings.
    let rl = &manifest.realmlist;
    let file = root.join(rl.file.replace("{locale}", &state.locale));
    let line = rl.line.replace("{address}", address);
    files::write_if_changed(&file, format!("{line}\r\n").as_bytes())?;
    report.realmlist = file;
    if let Some(saved) = &rl.saved {
        let path = root.join(&saved.file);
        if path.is_file() {
            let text = std::fs::read_to_string(&path).map_err(ClientError::io(&path))?;
            let prefix = saved.prefix.to_ascii_lowercase();
            let wanted = saved.line.replace("{address}", address);
            let mut changed = false;
            let lines: Vec<String> = text
                .lines()
                .map(|l| {
                    if l.to_ascii_lowercase().starts_with(&prefix) && l != wanted {
                        changed = true;
                        wanted.clone()
                    } else {
                        l.to_owned()
                    }
                })
                .collect();
            if changed {
                let mut out = lines.join("\r\n");
                out.push_str("\r\n");
                files::write_atomic(&path, out.as_bytes())?;
            }
        }
    }

    // The cache holds what the previous server sent (items, creatures).
    if state.last_server.as_deref() != Some(server) {
        if let Some(cache) = &manifest.cache {
            let path = root.join(&cache.directory);
            if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(ClientError::io(&path))?;
                report.cache_cleared = true;
            }
        }
        state.last_server = Some(server.to_owned());
        save(dir, state)?;
    }

    let patches = &manifest.patches.directory;
    let mut wanted: Vec<(PathBuf, String)> = files
        .iter()
        .map(|(src, to)| (src.clone(), format!("{patches}/{to}")))
        .collect();
    match &manifest.addons {
        Some(dir) => wanted.extend(
            addons
                .iter()
                .map(|(src, to)| (src.clone(), format!("{}/{to}", dir.directory))),
        ),
        None if !addons.is_empty() => report
            .warnings
            .push("this client has no add-on folder: add-ons skipped".into()),
        None => {}
    }
    files::reconcile(dir, state, &wanted, &mut report)?;
    Ok(report)
}

/// Starts the client from its folder. The child is independent of the
/// launcher (closing the launcher never closes the game).
pub fn launch(
    manifest: &ClientManifest,
    state: &ClientState,
) -> Result<tokio::process::Child, ClientError> {
    let exe = state.path.join(&manifest.detection.executable);
    tokio::process::Command::new(&exe)
        .current_dir(&state.path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(ClientError::io(&exe))
}

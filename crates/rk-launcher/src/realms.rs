//! Several realms and moving to another computer (ADR 0020).
//!
//! - **Duplicate**: a cold copy of a stopped realm under a new id, to try
//!   other modules without risking the main game. Server files and the SQL
//!   tree are hard-linked (they are replaced whole by an update, never edited
//!   in place); the database folder is copied without its server identity
//!   (`auto.cnf`, temporary tablespaces); the copy gets its own database port
//!   and its configs are written again by the module operation.
//! - **Export**: one `.rkrealm` file (a ZIP) holding `realm.json` (emulator,
//!   flavor, modules, options, name), `bindings.json` (accounts and
//!   characters that module settings name) and `player-data.sql` (a dump of
//!   the player databases). Content databases are rebuilt by the build.
//! - **Import**: a new realm installed with the exported choices, then the
//!   player databases restored from the dump (migrated by dbimport), the
//!   bindings put back and the configs written again. Resumable:
//!   `import.json` in the instance folder says what is left, and
//!   [`Launcher::resume`] finishes it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rk_engine::install::{self, InstanceRecord, Network};
use rk_engine::{CancelToken, Event, Observer};
use serde::{Deserialize, Serialize};

use crate::{
    backups, io_error, unix_secs, InstallRequest, Launcher, LauncherError, Options, Result,
};

/// Extension of an exported realm file.
pub const EXPORT_EXTENSION: &str = "rkrealm";
/// Format of `realm.json` in an export.
const EXPORT_FORMAT: u32 = 1;
const REALM_ENTRY: &str = "realm.json";
const BINDINGS_ENTRY: &str = "bindings.json";
const DUMP_ENTRY: &str = "player-data.sql";
/// Largest `realm.json` / `bindings.json` read from an export.
const SMALL_ENTRY_LIMIT: u64 = 1 << 20;
/// Longest realm name the player can give.
pub const NAME_LIMIT: usize = 48;

/// `realm.json` of an export: what the realm is, without anything tied to
/// this computer (ports, paths, passwords).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedRealm {
    pub format: u32,
    pub emulator: String,
    pub flavor: String,
    pub modules: Vec<String>,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub name: Option<String>,
    pub network: Network,
    /// Build the data comes from (informational: the import installs the
    /// current one and dbimport migrates the player data to it).
    #[serde(default)]
    pub build: Option<String>,
    /// Unix seconds.
    pub exported: u64,
    /// Launcher version that wrote the file.
    pub launcher: String,
}

/// What an export file holds, read before importing it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub realm: ExportedRealm,
    /// Size of the player data dump, in bytes.
    pub dump_size: u64,
}

/// `import.json`: an import still to finish (ADR 0020).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingImport {
    /// Backup file holding the exported player data.
    dump: String,
    options: Options,
    name: Option<String>,
    bindings: Option<serde_json::Value>,
    /// The player databases were put back: only bindings and configs remain.
    restored: bool,
}

/// A realm name as given by the player: trimmed, one line, not too long;
/// empty means none.
pub fn clean_name(name: Option<&str>) -> Result<Option<String>> {
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    if name.chars().count() > NAME_LIMIT || name.chars().any(char::is_control) {
        return Err(LauncherError::InvalidName(name.to_owned()));
    }
    Ok(Some(name.to_owned()))
}

impl Launcher {
    fn import_path(&self, id: &str) -> PathBuf {
        self.layout.instance(id).root.join("import.json")
    }

    fn pending_import(&self, id: &str) -> Result<Option<PendingImport>> {
        let path = self.import_path(id);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| LauncherError::Invalid(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(path, e)),
        }
    }

    fn save_import(&self, id: &str, pending: &PendingImport) -> Result<()> {
        let path = self.import_path(id);
        let dir = path.parent().expect("instance folder");
        std::fs::create_dir_all(dir).map_err(|e| io_error(dir.to_path_buf(), e))?;
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(pending).expect("serializable");
        std::fs::write(&tmp, json).map_err(|e| io_error(tmp.clone(), e))?;
        std::fs::rename(&tmp, &path).map_err(|e| io_error(path, e))
    }

    /// Whether an import of `id` is still to finish.
    pub(crate) fn import_unfinished(&self, id: &str) -> bool {
        self.import_path(id).is_file()
    }

    /// Removes what an import left when it stopped before its realm
    /// existed (no `instance.json`): its `import.json`, its extracted dump
    /// and the folders they made. Nothing else is ever there at that point.
    /// Returns whether `id` is free again.
    fn clear_unstarted_import(&self, id: &str) -> bool {
        let root = self.layout.instance(id).root;
        if root.join("instance.json").exists() || !self.import_path(id).is_file() {
            return false;
        }
        if let Ok(Some(pending)) = self.pending_import(id) {
            let dump = self.layout.backups(id).join(&pending.dump);
            let _ = std::fs::remove_file(dump);
            let _ = std::fs::remove_dir(self.layout.backups(id));
        }
        let _ = std::fs::remove_file(self.import_path(id));
        // Only if empty: never a folder holding anything else.
        std::fs::remove_dir(&root).is_ok() || !root.exists()
    }

    /// Fails when `id` is not a valid new realm id or is taken.
    fn check_new_id(&self, id: &str) -> Result<()> {
        if !install::valid_id(id) {
            return Err(install::PlanError::InvalidId(id.to_owned()).into());
        }
        // An import that stopped before its realm existed (crash) is not a
        // realm: it does not keep the id.
        self.clear_unstarted_import(id);
        if self.layout.instance(id).root.exists() {
            return Err(LauncherError::InstanceExists(id.to_owned()));
        }
        Ok(())
    }

    /// Gives a realm a name (`None` removes it), shown instead of its id.
    pub fn rename(&self, id: &str, name: Option<&str>) -> Result<InstanceRecord> {
        let name = clean_name(name)?;
        let _lock = install::lock_instance(&self.layout, id)?;
        let (mut record, _) = install::open_existing(&self.layout, id)?;
        record.name = name;
        install::save_record(&self.layout, &record)?;
        Ok(record)
    }

    // ------------------------------------------------------------ duplicate

    /// Copies the stopped realm `source` to a new realm `id` (ADR 0020):
    /// same build, modules, options, accounts and characters, its own
    /// database port. A failed copy leaves nothing behind.
    pub async fn duplicate(
        &self,
        source: &str,
        id: &str,
        name: Option<&str>,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let name = clean_name(name)?;
        self.check_new_id(id)?;
        let summary = self.instance(source)?;
        if !summary.installed || summary.unfinished.is_some() {
            return Err(install::PlanError::Unfinished(source.to_owned()).into());
        }
        let _source_lock = install::lock_instance(&self.layout, source)?;
        let (record, secrets) = install::open_existing(&self.layout, source)?;
        // A database left running (adopted, ADR 0011) must stop before its
        // files are copied: a live copy would be torn.
        let plan = install::instance_plan(&self.catalog, &self.layout, &record, &secrets)?;
        plan.db.stop().await?;

        // A copy an earlier duplicate left behind (crash) takes room.
        self.remove_trash();
        let from = self.layout.instance(source);
        let needed = dir_size(&from.db).saturating_add(dir_size(&from.configs));
        rk_store::ensure_space(self.layout.root(), needed).map_err(LauncherError::NoSpace)?;

        // Built aside, then renamed into place: a half copy is never a realm.
        let staging = self
            .layout
            .instances()
            .join(format!(".copy-{id}-{}", unix_secs()));
        let copied = tokio::task::block_in_place(|| {
            copy_instance(&from.root, &staging, needed, observer, cancel)
        });
        if let Err(e) = copied {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        let target = self.layout.instance(id).root;
        let mut copy = record.clone();
        copy.id = id.to_owned();
        copy.name = name.or_else(|| record.name.clone());
        copy.created = unix_secs();
        copy.regenerate.clear();
        copy.db_port = install::allocate_db_port(&self.layout)?;
        let written = (|| -> Result<()> {
            let json = serde_json::to_vec_pretty(&copy).expect("serializable");
            let path = staging.join("instance.json");
            std::fs::write(&path, json).map_err(|e| io_error(path, e))?;
            std::fs::rename(&staging, &target).map_err(|e| io_error(target.clone(), e))
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        drop(_source_lock);

        // The copy's own port in my.ini / client.cnf, then every config,
        // the server data overlay and module files at its place.
        let finished = async {
            let _lock = install::lock_instance(&self.layout, id)?;
            let (copy, secrets) = install::open_existing(&self.layout, id)?;
            let plan = install::instance_plan(&self.catalog, &self.layout, &copy, &secrets)?;
            plan.db.write_config()?;
            self.apply(copy.clone(), &copy, observer, cancel).await
        }
        .await;
        if finished.is_err() {
            let engine = rk_engine::Engine::new(self.layout.state());
            let _ = engine.forget(&install::modules_operation_id(id));
            let trash = self
                .layout
                .instances()
                .join(format!(".removed-{id}-{}", unix_secs()));
            if std::fs::rename(&target, &trash).is_ok() {
                let _ = std::fs::remove_dir_all(&trash);
            }
        }
        finished
    }

    // --------------------------------------------------------------- export

    /// Writes the stopped realm `id` to `out` (an `.rkrealm` file): its
    /// choices, bindings and player data. The database is started for the
    /// dump and stopped again.
    pub async fn export(&self, id: &str, out: &Path) -> Result<ExportSummary> {
        let summary = self.instance(id)?;
        if !summary.installed || summary.unfinished.is_some() {
            return Err(install::PlanError::Unfinished(id.to_owned()).into());
        }
        let _lock = install::lock_instance(&self.layout, id)?;
        let record = summary.record;
        let realm = ExportedRealm {
            format: EXPORT_FORMAT,
            emulator: record.emulator.clone(),
            flavor: record.flavor.clone(),
            modules: record.modules.clone(),
            options: record.options.clone(),
            name: record.name.clone(),
            network: record.network,
            build: record.build.clone(),
            exported: unix_secs(),
            launcher: env!("CARGO_PKG_VERSION").to_owned(),
        };
        // In the instance folder, not with the backups (it is not one).
        let dump = self.layout.instance(id).root.join("export.sql.tmp");
        let dumped = self.dump_player_data_to(id, &dump, false).await;
        let written = dumped.and_then(|()| {
            let bindings = self.layout.instance(id).bindings;
            let bindings = match std::fs::read(&bindings) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(io_error(bindings, e)),
            };
            tokio::task::block_in_place(|| write_export(out, &realm, bindings.as_deref(), &dump))
        });
        let _ = std::fs::remove_file(&dump);
        let dump_size = written?;
        Ok(ExportSummary { realm, dump_size })
    }

    // --------------------------------------------------------------- import

    /// Reads what an export file holds (nothing is changed).
    pub fn read_export(&self, file: &Path) -> Result<ExportSummary> {
        let (realm, _, dump_size) = read_export_head(file)?;
        self.check_importable(&realm)?;
        Ok(ExportSummary { realm, dump_size })
    }

    /// The exported emulator, flavor and modules exist in this catalog.
    fn check_importable(&self, realm: &ExportedRealm) -> Result<()> {
        let unsupported = |what: String| LauncherError::ExportUnsupported(what);
        let entry = self
            .catalog
            .emulator(&realm.emulator)
            .ok_or_else(|| unsupported(realm.emulator.clone()))?;
        if !entry.flavors.contains_key(&realm.flavor) {
            return Err(unsupported(format!("{} {}", realm.emulator, realm.flavor)));
        }
        if let Some(module) = realm
            .modules
            .iter()
            .find(|m| !entry.modules.contains_key(*m))
        {
            return Err(unsupported(module.clone()));
        }
        Ok(())
    }

    /// Installs a new realm `id` from an export file and puts its player
    /// data back (ADR 0020). Resumable with [`Launcher::resume`].
    pub async fn import(
        &self,
        file: &Path,
        id: &str,
        name: Option<&str>,
        index: Option<&str>,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let (realm, bindings, _) = read_export_head(file)?;
        self.check_importable(&realm)?;
        let name = match clean_name(name)? {
            Some(name) => Some(name),
            None => clean_name(realm.name.as_deref()).ok().flatten(),
        };
        self.check_new_id(id)?;
        // The player data goes to the new realm's backups: listed there,
        // and restorable again if the import is ever undone.
        let dump_name = format!("import-{}.sql", unix_secs());
        let backups_dir = self.layout.backups(id);
        std::fs::create_dir_all(&backups_dir).map_err(|e| io_error(backups_dir.clone(), e))?;
        let dump = backups_dir.join(&dump_name);
        tokio::task::block_in_place(|| extract_dump(file, &dump))?;
        self.save_import(
            id,
            &PendingImport {
                dump: dump_name,
                options: realm.options.clone(),
                name,
                bindings,
                restored: false,
            },
        )?;
        let request = InstallRequest {
            id: id.to_owned(),
            emulator: realm.emulator.clone(),
            flavor: Some(realm.flavor.clone()),
            modules: realm.modules.clone(),
            network: realm.network,
            server: None,
            db_snapshot: None,
            index: index.map(str::to_owned),
        };
        if let Err(error) = self.install(&request, observer, cancel).await {
            // Stopped before the realm existed (download failed, cancelled,
            // no room): nothing to resume, the id must stay free. Once the
            // realm exists, `import.json` makes `resume` finish the import.
            self.clear_unstarted_import(id);
            return Err(error);
        }
        self.finish_import(id, observer, cancel).await
    }

    /// The steps of an import after its install: options and name, player
    /// data, bindings, configs. Each is done once (`import.json` records it).
    pub(crate) async fn finish_import(
        &self,
        id: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let Some(mut pending) = self.pending_import(id)? else {
            return Ok(install::open_existing(&self.layout, id)?.0);
        };
        let _lock = install::lock_instance(&self.layout, id)?;
        let (mut record, _) = install::open_existing(&self.layout, id)?;
        if record.options != pending.options || record.name != pending.name {
            record.options = pending.options.clone();
            record.name = pending.name.clone();
            install::save_record(&self.layout, &record)?;
        }
        if !pending.restored {
            let dump = backups::resolve(&self.layout.backups(id), &pending.dump)?;
            self.restore_locked(record.clone(), dump, &pending.dump, observer, cancel)
                .await?;
            pending.restored = true;
            self.save_import(id, &pending)?;
        }
        // The exported bindings name the accounts and characters of the
        // restored data, not the ones the install made.
        let paths = self.layout.instance(id);
        match &pending.bindings {
            Some(bindings) => {
                let json = serde_json::to_vec_pretty(bindings).expect("serializable");
                std::fs::write(&paths.bindings, json)
                    .map_err(|e| io_error(paths.bindings.clone(), e))?;
            }
            None => match std::fs::remove_file(&paths.bindings) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(io_error(paths.bindings.clone(), e))
                }
                _ => {}
            },
        }
        rk_engine::Engine::new(self.layout.state()).forget(&install::modules_operation_id(id))?;
        let record = self
            .apply(record.clone(), &record, observer, cancel)
            .await?;
        let path = self.import_path(id);
        std::fs::remove_file(&path).map_err(|e| io_error(path, e))?;
        Ok(record)
    }
}

// ------------------------------------------------------------- copy helpers

/// Total size of the files under `dir` (0 when unreadable).
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// What happens to one entry of an instance folder in a copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyMode {
    /// Hard link (server files and SQL: replaced whole, never edited).
    Link,
    Copy,
    Skip,
}

/// How an entry at `relative` (with `/`) is copied.
fn copy_mode(relative: &str) -> CopyMode {
    let top = relative.split('/').next().unwrap_or_default();
    match top {
        "bin" | "sql" => CopyMode::Link,
        // Rebuilt for the copy: logs, the server data overlay (junctions),
        // the lock, an unfinished change's inputs.
        "logs" | "data" | "instance.lock" | "pending.json" | "import.json" => CopyMode::Skip,
        _ if top.ends_with(".tmp") => CopyMode::Skip,
        "db" => {
            let name = relative.rsplit('/').next().unwrap_or_default();
            // The server identity (a new UUID is made at start), temporary
            // tablespaces, the process id and the log of the original.
            let skip = name == "auto.cnf"
                || name.ends_with(".pid")
                || name == "mysqld.log"
                || relative.contains("/#innodb_temp")
                || (relative.contains("/#innodb_redo/") && name.ends_with("_tmp"));
            if skip {
                CopyMode::Skip
            } else {
                CopyMode::Copy
            }
        }
        _ => CopyMode::Copy,
    }
}

/// Copies an instance folder to `to` (created), reporting bytes copied.
fn copy_instance(
    from: &Path,
    to: &Path,
    total: u64,
    observer: &dyn Observer,
    cancel: &CancelToken,
) -> Result<()> {
    const STEP: &str = "copy";
    observer.event(&Event::StepStarted {
        index: 0,
        total: 1,
        step: STEP,
        kind: "copy-realm",
        resumed: false,
    });
    let mut done = 0u64;
    copy_tree(from, to, "", &mut |bytes| {
        if cancel.is_cancelled() {
            return Err(LauncherError::Cancelled("duplicate".into()));
        }
        done += bytes;
        observer.event(&Event::StepProgress {
            step: STEP,
            done,
            total: Some(total),
        });
        Ok(())
    })?;
    observer.event(&Event::StepFinished {
        index: 0,
        total: 1,
        step: STEP,
    });
    Ok(())
}

fn copy_tree(
    from: &Path,
    to: &Path,
    prefix: &str,
    progress: &mut dyn FnMut(u64) -> Result<()>,
) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| io_error(to.to_path_buf(), e))?;
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .map_err(|e| io_error(from.to_path_buf(), e))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let mode = copy_mode(&relative);
        if mode == CopyMode::Skip {
            continue;
        }
        let source = entry.path();
        let target = to.join(&name);
        let kind = entry.file_type().map_err(|e| io_error(source.clone(), e))?;
        if kind.is_dir() {
            copy_tree(&source, &target, &relative, progress)?;
        } else if kind.is_file() {
            let linked = mode == CopyMode::Link && std::fs::hard_link(&source, &target).is_ok();
            if !linked {
                let bytes =
                    std::fs::copy(&source, &target).map_err(|e| io_error(source.clone(), e))?;
                progress(bytes)?;
            }
        }
        // Junctions and other links are not copied: the module operation
        // makes the copy's own.
    }
    Ok(())
}

// ------------------------------------------------------------ export files

fn invalid_export(file: &Path, why: impl std::fmt::Display) -> LauncherError {
    LauncherError::ExportInvalid(format!("{}: {why}", file.display()))
}

/// Writes the export ZIP atomically; returns the dump size.
fn write_export(
    out: &Path,
    realm: &ExportedRealm,
    bindings: Option<&[u8]>,
    dump: &Path,
) -> Result<u64> {
    let tmp = out.with_extension(format!("{EXPORT_EXTENSION}.tmp"));
    let result = (|| -> std::result::Result<u64, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        zip.start_file(REALM_ENTRY, options)?;
        zip.write_all(&serde_json::to_vec_pretty(realm)?)?;
        if let Some(bindings) = bindings {
            zip.start_file(BINDINGS_ENTRY, options)?;
            zip.write_all(bindings)?;
        }
        zip.start_file(DUMP_ENTRY, options)?;
        let size = std::io::copy(&mut std::fs::File::open(dump)?, &mut zip)?;
        zip.finish()?.sync_all()?;
        Ok(size)
    })();
    match result {
        Ok(size) => {
            std::fs::rename(&tmp, out).map_err(|e| io_error(out.to_path_buf(), e))?;
            Ok(size)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(LauncherError::Invalid(format!("{}: {e}", out.display())))
        }
    }
}

fn open_export(file: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let handle = std::fs::File::open(file).map_err(|e| io_error(file.to_path_buf(), e))?;
    zip::ZipArchive::new(handle).map_err(|e| invalid_export(file, e))
}

/// A small entry of an export: `None` only when the file has no such entry.
/// A damaged one is an error, never taken for a missing one.
fn read_small(
    zip: &mut zip::ZipArchive<std::fs::File>,
    file: &Path,
    name: &str,
) -> Result<Option<Vec<u8>>> {
    let entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(invalid_export(file, format!("{name}: {e}"))),
    };
    let mut bytes = Vec::new();
    entry
        .take(SMALL_ENTRY_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid_export(file, format!("{name}: {e}")))?;
    Ok(Some(bytes))
}

/// `realm.json`, `bindings.json` and the dump size of an export.
fn read_export_head(file: &Path) -> Result<(ExportedRealm, Option<serde_json::Value>, u64)> {
    let mut zip = open_export(file)?;
    let realm = read_small(&mut zip, file, REALM_ENTRY)?
        .ok_or_else(|| invalid_export(file, format!("no {REALM_ENTRY}")))?;
    let realm: ExportedRealm =
        serde_json::from_slice(&realm).map_err(|e| invalid_export(file, e))?;
    if realm.format != EXPORT_FORMAT {
        return Err(invalid_export(
            file,
            format!(
                "format {} (this launcher reads {EXPORT_FORMAT})",
                realm.format
            ),
        ));
    }
    let bindings = match read_small(&mut zip, file, BINDINGS_ENTRY)? {
        Some(bytes) => Some(serde_json::from_slice(&bytes).map_err(|e| invalid_export(file, e))?),
        None => None,
    };
    let dump_size = zip
        .by_name(DUMP_ENTRY)
        .map_err(|_| invalid_export(file, format!("no {DUMP_ENTRY}")))?
        .size();
    Ok((realm, bindings, dump_size))
}

/// Unpacks the player data dump of an export to `out` (atomically). A
/// dump that does not end like a finished one (a truncated download) is
/// refused here, before anything is installed.
fn extract_dump(file: &Path, out: &Path) -> Result<()> {
    let mut zip = open_export(file)?;
    let mut entry = zip
        .by_name(DUMP_ENTRY)
        .map_err(|_| invalid_export(file, format!("no {DUMP_ENTRY}")))?;
    if let Some(dir) = out.parent() {
        rk_store::ensure_space(dir, entry.size()).map_err(LauncherError::NoSpace)?;
    }
    let tmp = out.with_extension("sql.tmp");
    let copied = (|| -> std::io::Result<()> {
        let mut target = std::fs::File::create(&tmp)?;
        std::io::copy(&mut entry, &mut target)?;
        target.flush()?;
        target.sync_all()
    })();
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_error(out.to_path_buf(), e));
    }
    if !rk_db::dump_complete(&tmp).unwrap_or(false) {
        let _ = std::fs::remove_file(&tmp);
        return Err(invalid_export(file, format!("{DUMP_ENTRY} is incomplete")));
    }
    std::fs::rename(&tmp, out).map_err(|e| io_error(out.to_path_buf(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_link_server_files_and_skip_what_is_rebuilt() {
        assert_eq!(copy_mode("bin/worldserver.exe"), CopyMode::Link);
        assert_eq!(copy_mode("sql/base/auth.sql"), CopyMode::Link);
        assert_eq!(copy_mode("configs/worldserver.conf"), CopyMode::Copy);
        assert_eq!(copy_mode("bindings.json"), CopyMode::Copy);
        assert_eq!(copy_mode("db/data/ibdata1"), CopyMode::Copy);
        assert_eq!(copy_mode("db/client.cnf"), CopyMode::Copy);
        for skipped in [
            "logs/Server.log",
            "data",
            "instance.lock",
            "pending.json",
            "import.json",
            "db/data/auto.cnf",
            "db/data/host.pid",
            "db/mysqld.log",
            "db/data/#innodb_temp",
            "db/data/#innodb_redo/#ib_redo267_tmp",
        ] {
            assert_eq!(copy_mode(skipped), CopyMode::Skip, "{skipped}");
        }
        assert_eq!(
            copy_mode("db/data/#innodb_redo/#ib_redo266"),
            CopyMode::Copy
        );
    }

    #[test]
    fn a_copy_holds_the_files_and_links_the_binaries() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("one");
        for (path, text) in [
            ("bin/worldserver.exe", "exe"),
            ("db/data/ibdata1", "data"),
            ("db/data/auto.cnf", "uuid"),
            ("logs/Server.log", "log"),
            ("secrets.json", "{}"),
        ] {
            let path = from.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let to = dir.path().join("two");
        copy_instance(&from, &to, 10, &(), &CancelToken::new()).unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("db/data/ibdata1")).unwrap(),
            "data"
        );
        assert!(to.join("secrets.json").is_file());
        assert!(!to.join("db/data/auto.cnf").exists());
        assert!(!to.join("logs").exists());
        // A hard link: writing through one name shows in the other.
        std::fs::write(from.join("bin/worldserver.exe"), "new").unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("bin/worldserver.exe")).unwrap(),
            "new"
        );
    }

    #[test]
    fn an_export_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("dump.sql");
        std::fs::write(&dump, "-- player data\n-- Dump completed on 2026-11-19\n").unwrap();
        let realm = ExportedRealm {
            format: EXPORT_FORMAT,
            emulator: "azerothcore".into(),
            flavor: "playerbot".into(),
            modules: vec!["mod-ah-bot".into()],
            options: Options::new(),
            name: Some("Test".into()),
            network: Network::Solo,
            build: Some("b".into()),
            exported: 1,
            launcher: "0".into(),
        };
        let out = dir.path().join("realm.rkrealm");
        let size = write_export(
            &out,
            &realm,
            Some(br#"{"mod-ah-bot":{"account":3}}"#),
            &dump,
        )
        .unwrap();
        assert_eq!(size, 47);
        let (read, bindings, dump_size) = read_export_head(&out).unwrap();
        assert_eq!(read, realm);
        assert_eq!(bindings.unwrap()["mod-ah-bot"]["account"], 3);
        assert_eq!(dump_size, 47);
        let back = dir.path().join("back.sql");
        extract_dump(&out, &back).unwrap();
        assert_eq!(
            std::fs::read_to_string(back).unwrap(),
            "-- player data\n-- Dump completed on 2026-11-19\n"
        );
    }

    #[test]
    fn a_file_that_is_not_an_export_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.rkrealm");
        std::fs::write(&file, "not a zip").unwrap();
        assert!(matches!(
            read_export_head(&file),
            Err(LauncherError::ExportInvalid(_))
        ));
    }

    fn test_launcher(root: &Path) -> Launcher {
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        Launcher::open(&catalog, root).unwrap()
    }

    fn new_realm(launcher: &Launcher, id: &str) {
        install::open_or_create(
            launcher.catalog(),
            launcher.layout(),
            &install::NewInstance {
                id: id.into(),
                emulator: "azerothcore".into(),
                flavor: None,
                modules: Vec::new(),
                network: Network::Solo,
            },
        )
        .unwrap();
    }

    /// An import that stopped before its realm existed (failed download,
    /// crash) keeps neither the id nor its extracted dump.
    #[test]
    fn an_import_stopped_before_its_realm_frees_the_id() {
        let root = tempfile::tempdir().unwrap();
        let launcher = test_launcher(root.path());
        let pending = PendingImport {
            dump: "import-1.sql".into(),
            options: Options::new(),
            name: None,
            bindings: None,
            restored: false,
        };
        launcher.save_import("fresh", &pending).unwrap();
        let backups = launcher.layout().backups("fresh");
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::write(backups.join("import-1.sql"), "--").unwrap();

        launcher.check_new_id("fresh").unwrap();
        assert!(!launcher.layout().instance("fresh").root.exists());
        assert!(!backups.exists());

        // Once the realm exists, `import.json` is what resume finishes.
        new_realm(&launcher, "made");
        launcher.save_import("made", &pending).unwrap();
        assert!(matches!(
            launcher.check_new_id("made"),
            Err(LauncherError::InstanceExists(_))
        ));
        assert!(launcher.import_unfinished("made"));
    }

    /// A copy being built, or a removal under way, holds an `instance.json`
    /// but is not a realm; the next duplicate clears what a crash left.
    #[test]
    fn copies_and_removals_under_way_are_not_realms() {
        let root = tempfile::tempdir().unwrap();
        let launcher = test_launcher(root.path());
        new_realm(&launcher, "one");
        let instances = launcher.layout().instances();
        for leftover in [".copy-two-1", ".removed-three-1"] {
            let dir = instances.join(leftover);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::copy(
                instances.join("one/instance.json"),
                dir.join("instance.json"),
            )
            .unwrap();
        }
        let ids: Vec<String> = launcher
            .instances()
            .unwrap()
            .into_iter()
            .map(|s| s.record.id)
            .collect();
        assert_eq!(ids, ["one"]);
        launcher.remove_trash();
        assert!(!instances.join(".copy-two-1").exists());
        assert!(!instances.join(".removed-three-1").exists());
    }

    /// A damaged `bindings.json` is an error, not a missing one (which would
    /// import the realm without its AHBot account, silently).
    #[test]
    fn a_damaged_bindings_entry_is_refused() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("damaged.rkrealm");
        let realm = br#"{"format":1}"#;
        let bindings = br#"{"mod-ah-bot":{"account":3}}"#;
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(REALM_ENTRY, stored).unwrap();
            zip.write_all(realm).unwrap();
            zip.start_file(BINDINGS_ENTRY, stored).unwrap();
            zip.write_all(bindings).unwrap();
            zip.finish().unwrap();
        }
        // Flip one byte of the stored bindings: its checksum no longer matches.
        let mut bytes = std::fs::read(&file).unwrap();
        let at = bytes
            .windows(bindings.len())
            .position(|w| w == bindings)
            .unwrap();
        bytes[at + 2] ^= 0x20;
        std::fs::write(&file, bytes).unwrap();

        let mut zip = open_export(&file).unwrap();
        assert!(read_small(&mut zip, &file, REALM_ENTRY).unwrap().is_some());
        assert!(read_small(&mut zip, &file, "absent.json")
            .unwrap()
            .is_none());
        assert!(matches!(
            read_small(&mut zip, &file, BINDINGS_ENTRY),
            Err(LauncherError::ExportInvalid(_))
        ));
    }

    #[test]
    fn names_are_one_short_line() {
        assert_eq!(
            clean_name(Some("  Test  ")).unwrap().as_deref(),
            Some("Test")
        );
        assert_eq!(clean_name(Some("   ")).unwrap(), None);
        assert_eq!(clean_name(None).unwrap(), None);
        assert!(clean_name(Some("a\nb")).is_err());
        assert!(clean_name(Some(&"x".repeat(NAME_LIMIT + 1))).is_err());
    }
}

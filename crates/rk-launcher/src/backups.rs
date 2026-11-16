//! Database backups of an instance: `backups/<id>/*.sql` (ADR 0016).
//!
//! - `manual-<secs>.sql`: made by the player ("Back up now"), kept until
//!   the player deletes it.
//! - `before-<label>-<secs>.sql`: made automatically before an update,
//!   repair or restore (the engine adds the time when the operation
//!   completes). Only the newest [`KEEP_AUTOMATIC`] are kept.
//! - `import-<secs>.sql`: the player data an imported realm came with
//!   (ADR 0020), kept until the player deletes it.
//! - `daily-<secs>.sql`: the player data, at most once a day when the
//!   servers stop (ADR 0018). Only the newest [`KEEP_DAILY`] are kept.
//!
//! A name without its time is the dump of an operation still in flight:
//! it is listed but never pruned.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{io_error, LauncherError, Result};

/// Automatic dumps kept per instance: enough to go back a few updates.
pub const KEEP_AUTOMATIC: usize = 5;
/// Daily backups kept per instance: a week of play.
pub const KEEP_DAILY: usize = 7;
/// A new daily backup once the last one is this old.
pub const DAILY_EVERY_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupKind {
    Manual,
    Daily,
    Update,
    Repair,
    Restore,
    /// Player data of an imported realm (ADR 0020), kept like a manual one.
    Import,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    /// File name, the backup's id.
    pub name: String,
    pub kind: BackupKind,
    /// Unix seconds: the time in the name, else the file's modification time.
    pub created: u64,
    pub size: u64,
    /// The newest automatic ones are kept, older ones deleted (false for
    /// manual backups and in-flight dumps).
    pub automatic: bool,
}

/// Name of a new manual backup.
pub fn manual_name(secs: u64) -> String {
    format!("manual-{secs}.sql")
}

/// Name of a new daily backup.
pub fn daily_name(secs: u64) -> String {
    format!("daily-{secs}.sql")
}

/// Whether a daily backup is due at `now`: none in the last day.
pub fn daily_due(dir: &Path, now: u64) -> Result<bool> {
    Ok(!list(dir)?
        .iter()
        .any(|b| b.kind == BackupKind::Daily && b.created.saturating_add(DAILY_EVERY_SECS) > now))
}

/// Backups in `dir`, newest first. A missing folder has none.
pub fn list(dir: &Path) -> Result<Vec<Backup>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(dir.to_path_buf(), e)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error(dir.to_path_buf(), e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".sql") else {
            continue;
        };
        let meta = entry.metadata().map_err(|e| io_error(entry.path(), e))?;
        if !meta.is_file() {
            continue;
        }
        let stamp = time_suffix(stem);
        let created = stamp.unwrap_or_else(|| {
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs())
        });
        let kind = kind_of(stem);
        out.push(Backup {
            automatic: (stem.starts_with("before-") || stem.starts_with("daily-"))
                && stamp.is_some(),
            name,
            kind,
            created,
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.name.cmp(&a.name)));
    Ok(out)
}

/// Deletes the automatic backups beyond the newest [`KEEP_DAILY`] daily
/// ones and the newest [`KEEP_AUTOMATIC`] others.
pub fn prune(dir: &Path) -> Result<()> {
    let automatic: Vec<Backup> = list(dir)?.into_iter().filter(|b| b.automatic).collect();
    let (daily, others): (Vec<&Backup>, Vec<&Backup>) =
        automatic.iter().partition(|b| b.kind == BackupKind::Daily);
    let old = daily
        .into_iter()
        .skip(KEEP_DAILY)
        .chain(others.into_iter().skip(KEEP_AUTOMATIC));
    for backup in old {
        let path = dir.join(&backup.name);
        std::fs::remove_file(&path).map_err(|e| io_error(path, e))?;
    }
    Ok(())
}

/// Path of the backup `name` in `dir`: a plain file name, listed there.
pub fn resolve(dir: &Path, name: &str) -> Result<PathBuf> {
    let plain = !name.is_empty()
        && name.ends_with(".sql")
        && !name.contains(['/', '\\', ':'])
        && name != ".."
        && !name.starts_with('.');
    let path = dir.join(name);
    if plain && path.is_file() {
        Ok(path)
    } else {
        Err(LauncherError::BackupNotFound(name.to_owned()))
    }
}

/// `...-<unix secs>` → the seconds (10 digits or so; at least 9).
fn time_suffix(stem: &str) -> Option<u64> {
    let (_, digits) = stem.rsplit_once('-')?;
    if digits.len() < 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn kind_of(stem: &str) -> BackupKind {
    if stem.starts_with("manual-") {
        BackupKind::Manual
    } else if stem.starts_with("daily-") {
        BackupKind::Daily
    } else if stem.starts_with("before-update-") {
        BackupKind::Update
    } else if stem.starts_with("before-repair-") {
        BackupKind::Repair
    } else if stem.starts_with("before-restore-") {
        BackupKind::Restore
    } else if stem.starts_with("import-") {
        BackupKind::Import
    } else {
        BackupKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"-- dump\n").unwrap();
    }

    #[test]
    fn lists_newest_first_and_prunes_only_old_automatic_dumps() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        touch(d, "manual-1700000000.sql");
        for i in 0..7 {
            touch(d, &format!("before-update-a-to-b-17000000{i}0.sql"));
        }
        touch(d, "before-update-x-to-y.sql"); // in flight: no time added yet
        touch(d, "notes.txt");
        let all = list(d).unwrap();
        assert_eq!(all.len(), 9);
        // The in-flight dump has no time in its name: its file time (now)
        // makes it the newest.
        assert_eq!(all[0].name, "before-update-x-to-y.sql");
        assert_eq!(all[1].name, "before-update-a-to-b-1700000060.sql");
        assert_eq!(all[1].kind, BackupKind::Update);

        for i in 0..9 {
            touch(d, &format!("daily-17000001{i}0.sql"));
        }
        prune(d).unwrap();
        let left = list(d).unwrap();
        assert_eq!(
            left.iter()
                .filter(|b| b.automatic && b.kind != BackupKind::Daily)
                .count(),
            KEEP_AUTOMATIC
        );
        assert_eq!(
            left.iter().filter(|b| b.kind == BackupKind::Daily).count(),
            KEEP_DAILY
        );
        assert!(!d.join("daily-1700000100.sql").exists());
        assert!(left.iter().any(|b| b.name == "manual-1700000000.sql"));
        assert!(left
            .iter()
            .any(|b| b.name == "before-update-x-to-y.sql" && !b.automatic));
        assert!(!d.join("before-update-a-to-b-1700000000.sql").exists());
    }

    #[test]
    fn a_daily_backup_is_due_once_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let now = 1_800_000_000;
        assert!(daily_due(dir.path(), now).unwrap());
        touch(dir.path(), &manual_name(now - 10));
        assert!(
            daily_due(dir.path(), now).unwrap(),
            "manual ones do not count"
        );
        touch(dir.path(), &daily_name(now - DAILY_EVERY_SECS + 60));
        assert!(!daily_due(dir.path(), now).unwrap());
        assert!(daily_due(dir.path(), now + 120).unwrap());
    }

    #[test]
    fn resolves_only_plain_listed_names() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "manual-1700000000.sql");
        assert!(resolve(dir.path(), "manual-1700000000.sql").is_ok());
        for bad in [
            "../x.sql",
            "a/b.sql",
            "C:x.sql",
            "missing.sql",
            "manual-1700000000",
        ] {
            assert!(matches!(
                resolve(dir.path(), bad),
                Err(LauncherError::BackupNotFound(_))
            ));
        }
    }
}

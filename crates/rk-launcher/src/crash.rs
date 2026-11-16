//! Opt-in crash reports (ADR 0015). A panic writes a report under
//! `<root>/crash-reports/`; nothing leaves the computer by itself. At the
//! next start the app offers to open a pre-filled GitHub issue, which the
//! player reads and submits (or not) on their own. Personal paths are
//! replaced by environment variable names before anything is shown.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Reports kept at most (older ones are deleted).
const KEEP: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CrashReport {
    /// File name, used to dismiss it.
    pub id: String,
    pub text: String,
}

pub fn dir(root: &Path) -> PathBuf {
    root.join("crash-reports")
}

/// Replaces the user's folders by their variable names, and the user name
/// wherever it still appears.
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for var in ["LOCALAPPDATA", "APPDATA", "USERPROFILE", "HOME"] {
        if let Some(value) = std::env::var_os(var) {
            let value = value.to_string_lossy();
            if value.len() > 3 {
                out = out.replace(value.as_ref(), &format!("%{var}%"));
                out = out.replace(&value.replace('\\', "/"), &format!("%{var}%"));
            }
        }
    }
    for var in ["USERNAME", "USER"] {
        if let Some(value) = std::env::var_os(var) {
            let value = value.to_string_lossy();
            if value.len() > 2 {
                out = out.replace(value.as_ref(), "<user>");
            }
        }
    }
    out
}

/// Writes a report for every panic (then the default hook runs).
pub fn install_hook(root: &Path, app_version: &'static str) {
    let dir = dir(root);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let text = format!(
            "RealmKeeper {app_version} ({} {})\nthread: {}\n{info}\n\nbacktrace:\n{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture(),
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(dir.join(format!("crash-{stamp}.txt")), redact(&text));
        }
        default(info);
    }));
}

/// Reports not dismissed yet, newest first (redacted again on read).
pub fn reports(root: &Path) -> Vec<CrashReport> {
    let Ok(entries) = std::fs::read_dir(dir(root)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        .collect();
    names.sort_by(|a, b| b.cmp(a));
    for old in names.iter().skip(KEEP) {
        let _ = std::fs::remove_file(dir(root).join(old));
    }
    names
        .into_iter()
        .take(KEEP)
        .filter_map(|id| {
            let text = std::fs::read_to_string(dir(root).join(&id)).ok()?;
            Some(CrashReport {
                text: redact(&text),
                id,
            })
        })
        .collect()
}

/// Deletes a report (sent or declined).
pub fn dismiss(root: &Path, id: &str) -> std::io::Result<()> {
    if !(id.starts_with("crash-") && id.ends_with(".txt")) || id.contains(['/', '\\']) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a crash report",
        ));
    }
    match std::fs::remove_file(dir(root).join(id)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_listed_capped_and_dismissed() {
        let root = tempfile::tempdir().unwrap();
        let dir = dir(root.path());
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..12 {
            std::fs::write(dir.join(format!("crash-{i:03}.txt")), format!("panic {i}")).unwrap();
        }
        let list = reports(root.path());
        assert_eq!(list.len(), KEEP);
        assert_eq!(list[0].id, "crash-011.txt");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), KEEP);
        dismiss(root.path(), "crash-011.txt").unwrap();
        assert_eq!(reports(root.path())[0].id, "crash-010.txt");
        assert!(dismiss(root.path(), "../instance.json").is_err());
    }

    #[test]
    fn home_folders_are_redacted() {
        let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
        else {
            return;
        };
        let home = home.to_string_lossy().into_owned();
        let text = redact(&format!("at {home}\\x.rs"));
        assert!(!text.contains(&home), "{text}");
    }
}

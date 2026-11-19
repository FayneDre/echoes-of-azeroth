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

/// Replaces the user's folders by their variable names, and the user and
/// computer names wherever they still appear. Windows paths are not case
/// sensitive (`c:\users\…` in one log, `C:\Users\…` in another), so the
/// matching is not either.
pub fn redact(text: &str) -> String {
    let var = |name: &str| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned());
    let mut out = text.to_owned();
    for name in ["LOCALAPPDATA", "APPDATA", "USERPROFILE", "HOME"] {
        if let Some(value) = var(name).filter(|v| v.len() > 3) {
            let by = format!("%{name}%");
            out = replace_ignore_case(&out, &value, &by);
            out = replace_ignore_case(&out, &value.replace('\\', "/"), &by);
        }
    }
    for (name, by) in [
        ("USERNAME", "<user>"),
        ("USER", "<user>"),
        ("COMPUTERNAME", "<computer>"),
    ] {
        if let Some(value) = var(name).filter(|v| v.len() > 2) {
            out = replace_ignore_case(&out, &value, by);
        }
    }
    out
}

/// Replaces every occurrence of `needle` in `text`, ASCII case ignored.
fn replace_ignore_case(text: &str, needle: &str, by: &str) -> String {
    if needle.is_empty() {
        return text.to_owned();
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    // ASCII lower-casing keeps every byte offset, so positions match.
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in lower_text.match_indices(&lower_needle) {
        if at < last {
            continue;
        }
        out.push_str(&text[last..at]);
        out.push_str(by);
        last = at + needle.len();
    }
    out.push_str(&text[last..]);
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
        let lower = home.to_ascii_lowercase();
        let text = redact(&format!("at {lower}\\x.rs"));
        assert!(!text.to_ascii_lowercase().contains(&lower), "{text}");
    }

    #[test]
    fn replacing_ignores_ascii_case_only() {
        assert_eq!(
            replace_ignore_case(r"C:\Users\Ann and c:\users\ann", r"c:\users\ann", "%H%"),
            "%H% and %H%"
        );
        assert_eq!(
            replace_ignore_case("Élodie élodie", "élodie", "<u>"),
            "Élodie <u>"
        );
        assert_eq!(replace_ignore_case("aaa", "aa", "b"), "ba");
    }
}

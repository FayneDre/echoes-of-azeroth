//! A support report the player copies into a bug report (ADR 0018): the
//! launcher version, the realms and their state, the game folder check and
//! the last lines of the server logs. Built on demand, never sent anywhere:
//! the player pastes it. Personal folders and the user name are redacted
//! like crash reports; no password or secret file is read.

use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rk_engine::install;
use rk_engine::Engine;

use crate::{backups, crash, Launcher};

/// Log files shown per realm (the most recently written).
const LOG_FILES: usize = 6;
/// Lines shown per log file.
const LOG_LINES: usize = 25;
/// Bytes read at the end of a log file (they can be large).
const LOG_TAIL_BYTES: u64 = 64 * 1024;

/// The report, redacted. `running` are the realms whose servers run.
pub fn report(launcher: &Launcher, app_version: &str, running: &[String]) -> String {
    let mut out = String::new();
    let _ = write_report(&mut out, launcher, app_version, running);
    crash::redact(&out)
}

fn write_report(
    out: &mut String,
    launcher: &Launcher,
    app_version: &str,
    running: &[String],
) -> std::fmt::Result {
    writeln!(out, "## RealmKeeper {app_version}")?;
    writeln!(
        out,
        "- platform: {} ({})",
        install::platform(),
        std::env::consts::FAMILY
    )?;
    let root = launcher.layout().root();
    writeln!(out, "- data root: {}", root.display())?;
    match rk_store::available_space(root) {
        Ok(free) => writeln!(out, "- free space: {:.1} GB", free as f64 / 1e9)?,
        Err(e) => writeln!(out, "- free space: unknown ({e})")?,
    }
    let crashes = crash::reports(&crate::app_dir()).len();
    writeln!(out, "- crash reports waiting: {crashes}")?;

    writeln!(out, "\n## Game clients")?;
    for (id, manifest) in &launcher.catalog().clients {
        match launcher.client(id) {
            Ok(None) => writeln!(out, "- {id}: not chosen")?,
            Ok(Some(state)) => {
                let check = match rk_client::detect(manifest, &state.path) {
                    Ok(found) => format!("ok (build {})", found.build),
                    Err(e) => format!("problem: {e}"),
                };
                writeln!(
                    out,
                    "- {id}: {} [{}], {} launcher file(s), {check}",
                    state.path.display(),
                    state.locale,
                    state.managed.len()
                )?;
            }
            Err(e) => writeln!(out, "- {id}: unreadable ({e})")?,
        }
    }

    writeln!(out, "\n## Realms")?;
    let instances = match launcher.instances() {
        Ok(list) => list,
        Err(e) => return writeln!(out, "unreadable: {e}"),
    };
    if instances.is_empty() {
        writeln!(out, "none installed")?;
    }
    let engine = Engine::new(launcher.layout().state());
    for summary in instances {
        let record = &summary.record;
        let id = &record.id;
        writeln!(out, "\n### {id}")?;
        writeln!(
            out,
            "- {} / {}, build {}, {:?}, ports {:?}, database port {}",
            record.emulator,
            record.flavor,
            record.build.as_deref().unwrap_or("none"),
            record.network,
            record.ports,
            record.db_port
        )?;
        writeln!(out, "- modules: {}", record.modules.join(", "))?;
        if !record.options.is_empty() {
            let options = serde_json::to_string(&record.options).unwrap_or_default();
            writeln!(out, "- options: {options}")?;
        }
        writeln!(
            out,
            "- installed: {}, running: {}, interrupted change: {}",
            summary.installed,
            running.contains(id),
            summary.unfinished.as_deref().unwrap_or("none")
        )?;
        for op in [
            install::instance_operation_id(id),
            install::modules_operation_id(id),
        ] {
            if let Ok(Some(journal)) = engine.status(&op) {
                write!(out, "- {op}: {:?}", journal.status)?;
                match &journal.failure {
                    Some(failure) => writeln!(out, " ({failure})")?,
                    None => writeln!(out)?,
                }
            }
        }
        if let Ok(list) = backups::list(&launcher.backups_dir(id)) {
            let newest = list.first().map_or("none", |b| b.name.as_str());
            writeln!(out, "- backups: {}, newest {newest}", list.len())?;
        }
        write_logs(out, &launcher.layout().instance(id).logs)?;
    }
    Ok(())
}

/// The last lines of the most recently written log files of a realm.
fn write_logs(out: &mut String, dir: &Path) -> std::fmt::Result {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut logs: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, path) in logs.into_iter().take(LOG_FILES) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let lines = tail(&path, LOG_LINES).unwrap_or_default();
        if lines.is_empty() {
            continue;
        }
        writeln!(out, "\n<details><summary>{name}</summary>\n\n```")?;
        for line in lines {
            writeln!(out, "{line}")?;
        }
        writeln!(out, "```\n</details>")?;
    }
    Ok(())
}

/// The last `count` lines of a text file, reading only its end.
fn tail(path: &Path, count: usize) -> std::io::Result<Vec<String>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // cut in the middle
    }
    let skip = lines.len().saturating_sub(count);
    Ok(lines[skip..].iter().map(|l| l.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_reads_the_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Server.log");
        let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&file, text).unwrap();
        assert_eq!(tail(&file, 2).unwrap(), ["line 98", "line 99"]);
    }

    #[test]
    fn a_fresh_launcher_reports_no_realm() {
        let root = tempfile::tempdir().unwrap();
        let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let launcher = Launcher::open(&catalog, root.path()).unwrap();
        let text = report(&launcher, "9.9.9", &[]);
        assert!(text.contains("RealmKeeper 9.9.9"));
        assert!(text.contains("none installed"));
        assert!(text.contains("wow-3-3-5a: not chosen"));
    }
}

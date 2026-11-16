//! Player preferences of the launcher itself (not of a realm), in
//! `<app dir>/preferences.json` (ADR 0018). A missing or unreadable file
//! gives the defaults: preferences never block the launcher.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "preferences.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    /// Back up the player data once a day, when the servers stop.
    pub daily_backup: bool,
    /// Hide the launcher in the notification area while the game runs.
    pub hide_while_playing: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            daily_backup: true,
            hide_while_playing: true,
        }
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

pub fn load(dir: &Path) -> Preferences {
    std::fs::read(path(dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, preferences: &Preferences) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let file = path(dir);
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(preferences)?)?;
    std::fs::rename(&tmp, &file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_then_saved_values() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Preferences::default());
        std::fs::write(dir.path().join(FILE), r#"{"dailyBackup": false}"#).unwrap();
        let read = load(dir.path());
        assert!(!read.daily_backup);
        assert!(read.hide_while_playing, "a missing key keeps its default");
        let changed = Preferences {
            daily_backup: true,
            hide_while_playing: false,
        };
        save(dir.path(), &changed).unwrap();
        assert_eq!(load(dir.path()), changed);
        std::fs::write(dir.path().join(FILE), "not json").unwrap();
        assert_eq!(load(dir.path()), Preferences::default());
    }
}

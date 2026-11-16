//! Looks for the game in the usual places so the player does not have to
//! browse for it (ADR 0018). Bounded: a few folder levels under each drive
//! and the player's own folders, descending only into folders whose name
//! suggests a game; at most [`MAX_FOLDERS`] folders are read. Only clients
//! [`detect`](crate::detect) accepts (right build) are returned.

use std::path::{Path, PathBuf};

use rk_core::manifest::ClientManifest;

use crate::{detect, Detected};

/// Folders read at most by one search.
pub const MAX_FOLDERS: usize = 6000;
/// Levels below a starting folder.
const MAX_DEPTH: usize = 4;

/// Names that suggest a game, a client or a games folder (lower case).
const HINTS: &[&str] = &[
    "wow", "warcraft", "wotlk", "lich", "3.3.5", "335", "client", "game", "jeu", "blizzard",
    "spiele", "juego", "gioch",
];

/// Folders never worth reading (lower case).
const SKIP: &[&str] = &[
    "windows",
    "$recycle.bin",
    "system volume information",
    "programdata",
    "appdata",
    "users",
    "recovery",
    "perflogs",
    "msocache",
    "config.msi",
    "windowsapps",
    "$windows.~bt",
    "$windows.~ws",
    "node_modules",
    ".git",
    "realmkeeper",
    "realmkeeperdata",
];

/// Clients of `manifest` found in the usual places, by path.
pub fn find(manifest: &ClientManifest) -> Vec<Detected> {
    let mut search = Search {
        manifest,
        budget: MAX_FOLDERS,
        found: Vec::new(),
    };
    for start in starting_points() {
        search.walk(&start, 0);
    }
    let mut found = search.found;
    found.sort_by(|a, b| a.root.cmp(&b.root));
    found.dedup_by(|a, b| a.root == b.root);
    found
}

/// Drive roots, then the player's own folders.
fn starting_points() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        // A and B are floppy letters; reading them can hang.
        for letter in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", letter as char));
            if root.is_dir() {
                out.push(root);
            }
        }
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    if let Some(home) = home.map(PathBuf::from) {
        for sub in [
            "Desktop",
            "Downloads",
            "Documents",
            "Games",
            "Bureau",
            "Téléchargements",
        ] {
            let dir = home.join(sub);
            if dir.is_dir() {
                out.push(dir);
            }
        }
    }
    out
}

struct Search<'a> {
    manifest: &'a ClientManifest,
    budget: usize,
    found: Vec<Detected>,
}

impl Search<'_> {
    fn walk(&mut self, dir: &Path, depth: usize) {
        if self.budget == 0 {
            return;
        }
        self.budget -= 1;
        if dir.join(&self.manifest.detection.executable).is_file() {
            if let Ok(found) = detect(self.manifest, dir) {
                self.found.push(found);
            }
            return; // never inside a game folder
        }
        if depth >= MAX_DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut children: Vec<(PathBuf, String)> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| (e.path(), e.file_name().to_string_lossy().to_lowercase()))
            .filter(|(_, name)| !SKIP.contains(&name.as_str()) && !name.starts_with('$'))
            .collect();
        children.sort();
        for (child, name) in children {
            // Every folder right under a starting point; deeper, only names
            // that suggest a game.
            if depth == 0 || HINTS.iter().any(|hint| name.contains(hint)) {
                self.walk(&child, depth + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> ClientManifest {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/clients/wow-3-3-5a.json");
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn descends_only_into_folders_that_suggest_a_game() {
        let root = tempfile::tempdir().unwrap();
        let wanted = root.path().join("Jeux/World of Warcraft/Clients/WotLK");
        let ignored = root.path().join("Photos/Holidays/Wow");
        for dir in [&wanted, &ignored] {
            std::fs::create_dir_all(dir).unwrap();
            // Not a real client: detect() refuses it, but the walk must
            // have looked at the folder (budget spent there).
            std::fs::write(dir.join("Wow.exe"), b"MZ").unwrap();
        }
        let manifest = manifest();
        let mut search = Search {
            manifest: &manifest,
            budget: MAX_FOLDERS,
            found: Vec::new(),
        };
        search.walk(root.path(), 0);
        // root, Jeux, World of Warcraft, Clients, WotLK, Photos: 6 folders;
        // "Holidays" does not suggest a game.
        assert_eq!(MAX_FOLDERS - search.budget, 6);
        assert!(search.found.is_empty(), "a fake executable is not a client");
    }

    #[test]
    fn the_budget_bounds_the_walk() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::create_dir_all(root.path().join(format!("game{i}/client"))).unwrap();
        }
        let manifest = manifest();
        let mut search = Search {
            manifest: &manifest,
            budget: 5,
            found: Vec::new(),
        };
        search.walk(root.path(), 0);
        assert_eq!(search.budget, 0);
    }
}

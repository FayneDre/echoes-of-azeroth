//! Whether a folder can hold a data root (ADR 0020): not inside a folder a
//! sync client uploads (a MySQL data directory synchronised while the server
//! writes it ends up corrupted), and on a file system that supports the
//! directory links of the server data overlay (NTFS; not FAT32 / exFAT or
//! most network shares).

use std::path::{Path, PathBuf};

use crate::{LauncherError, Result};

/// Folders known to be synchronised, with the client's name.
fn synced_folders() -> Vec<(PathBuf, &'static str)> {
    let mut folders = Vec::new();
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(path) = std::env::var_os(var).filter(|p| !p.is_empty()) {
            folders.push((PathBuf::from(path), "OneDrive"));
        }
    }
    // Dropbox writes its folders in `info.json` (personal, business).
    for base in ["APPDATA", "LOCALAPPDATA"] {
        let Some(base) = std::env::var_os(base) else {
            continue;
        };
        let info = PathBuf::from(base).join("Dropbox").join("info.json");
        if let Ok(bytes) = std::fs::read(info) {
            folders.extend(dropbox_folders(&bytes).into_iter().map(|p| (p, "Dropbox")));
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        folders.push((PathBuf::from(home).join("iCloudDrive"), "iCloud Drive"));
    }
    folders
}

fn dropbox_folders(info: &[u8]) -> Vec<PathBuf> {
    let Ok(serde_json::Value::Object(accounts)) = serde_json::from_slice(info) else {
        return Vec::new();
    };
    accounts
        .values()
        .filter_map(|account| account.get("path")?.as_str())
        .map(PathBuf::from)
        .collect()
}

/// Folder names Google Drive for desktop uses for its mirrored files.
const GOOGLE_DRIVE_NAMES: [&str; 3] = ["google drive", "my drive", "mon drive"];

/// The sync client that uploads `path`, if any.
pub fn synced_by(path: &Path) -> Option<&'static str> {
    synced_by_in(path, &synced_folders())
}

fn synced_by_in(path: &Path, folders: &[(PathBuf, &'static str)]) -> Option<&'static str> {
    let lower = |p: &Path| p.to_string_lossy().to_lowercase().replace('/', "\\");
    let here = lower(path);
    for (folder, client) in folders {
        let folder = lower(folder);
        let folder = folder.trim_end_matches('\\');
        if folder.is_empty() {
            continue;
        }
        if here == folder || here.starts_with(&format!("{folder}\\")) {
            return Some(client);
        }
    }
    here.split('\\')
        .any(|name| GOOGLE_DRIVE_NAMES.contains(&name))
        .then_some("Google Drive")
}

/// Fails when `root` (an existing folder) cannot hold a data root.
pub fn check(root: &Path) -> Result<()> {
    if let Some(client) = synced_by(root) {
        return Err(LauncherError::LocationSynced {
            path: root.to_path_buf(),
            client: client.to_owned(),
        });
    }
    rk_engine::install::assets::supports_dir_links(root).map_err(|e| {
        LauncherError::LocationUnsupported {
            path: root.to_path_buf(),
            reason: e.to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_under_a_sync_client_are_found() {
        let folders = vec![
            (PathBuf::from(r"C:\Users\p\OneDrive"), "OneDrive"),
            (PathBuf::from(r"D:\Dropbox\"), "Dropbox"),
        ];
        let synced = |p: &str| synced_by_in(Path::new(p), &folders);
        assert_eq!(synced(r"C:\Users\p\OneDrive\Games"), Some("OneDrive"));
        assert_eq!(synced(r"c:\users\p\onedrive"), Some("OneDrive"));
        assert_eq!(synced(r"C:\Users\p\OneDrive2\Games"), None);
        assert_eq!(synced(r"D:\Dropbox\WoW"), Some("Dropbox"));
        assert_eq!(synced(r"G:\My Drive\Realms"), Some("Google Drive"));
        assert_eq!(synced(r"D:\Games\Realms"), None);
    }

    #[test]
    fn dropbox_info_lists_every_account() {
        let info = br#"{"personal":{"path":"C:\\Users\\p\\Dropbox","host":1},"business":{"path":"D:\\Work Dropbox"}}"#;
        let mut folders = dropbox_folders(info);
        folders.sort();
        assert_eq!(
            folders,
            vec![
                PathBuf::from(r"C:\Users\p\Dropbox"),
                PathBuf::from(r"D:\Work Dropbox")
            ]
        );
        assert!(dropbox_folders(b"not json").is_empty());
    }

    #[test]
    fn a_local_folder_holds_links() {
        let dir = tempfile::tempdir().unwrap();
        check(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "probe removed"
        );
    }
}

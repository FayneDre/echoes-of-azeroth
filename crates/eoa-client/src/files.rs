//! Files the launcher puts in the player's client, tracked by sha256.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{save, ClientError, ClientState, PrepareReport};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedFile {
    /// Relative to the client root, `/` separators.
    pub path: String,
    /// What the launcher wrote.
    pub sha256: String,
    /// Where the player's own file at this path was moved, if there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
}

fn same_path(a: &str, b: &str) -> bool {
    // Windows paths and MPQ names are case-insensitive.
    a.eq_ignore_ascii_case(b)
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, ClientError> {
    let mut file = std::fs::File::open(path).map_err(ClientError::io(path))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(ClientError::io(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Same content on disk: `None` when the file does not exist.
fn hash_if_exists(path: &Path) -> Result<Option<String>, ClientError> {
    if path.is_file() {
        sha256_file(path).map(Some)
    } else {
        Ok(None)
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ClientError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(ClientError::io(dir))?;
    }
    let tmp = tmp_path(path);
    std::fs::write(&tmp, bytes).map_err(ClientError::io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(ClientError::io(path))
}

pub(crate) fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), ClientError> {
    match std::fs::read(path) {
        Ok(current) if current == bytes => Ok(()),
        _ => write_atomic(path, bytes),
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".eoa-tmp");
    path.with_file_name(name)
}

fn copy_atomic(from: &Path, to: &Path) -> Result<(), ClientError> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(ClientError::io(dir))?;
    }
    let tmp = tmp_path(to);
    std::fs::copy(from, &tmp).map_err(ClientError::io(from))?;
    std::fs::rename(&tmp, to).map_err(ClientError::io(to))
}

/// Makes the managed files exactly `wanted` (source, path relative to the
/// client root). Every change is saved in the state before it is made.
pub(crate) fn reconcile(
    dir: &Path,
    state: &mut ClientState,
    wanted: &[(PathBuf, String)],
    report: &mut PrepareReport,
) -> Result<(), ClientError> {
    let root = state.path.clone();

    // 1. Remove what is no longer wanted.
    let stale: Vec<ManagedFile> = state
        .managed
        .iter()
        .filter(|m| !wanted.iter().any(|(_, to)| same_path(to, &m.path)))
        .cloned()
        .collect();
    for m in stale {
        let full = root.join(&m.path);
        match hash_if_exists(&full)? {
            Some(hash) if hash == m.sha256 => {
                std::fs::remove_file(&full).map_err(ClientError::io(&full))?;
                report.removed.push(m.path.clone());
            }
            Some(_) => report.warnings.push(format!(
                "{} was changed outside the launcher: left in place",
                m.path
            )),
            None => {}
        }
        if let Some(backup) = &m.backup {
            let saved = root.join(backup);
            if saved.is_file() && !full.exists() {
                std::fs::rename(&saved, &full).map_err(ClientError::io(&saved))?;
            } else if saved.is_file() {
                report
                    .warnings
                    .push(format!("your previous {} is kept as {backup}", m.path));
            }
        }
        state.managed.retain(|x| !same_path(&x.path, &m.path));
        save(dir, state)?;
    }

    // 2. Add or refresh what is wanted.
    for (source, to) in wanted {
        let hash = sha256_file(source)?;
        let full = root.join(to);
        let on_disk = hash_if_exists(&full)?;
        let index = state.managed.iter().position(|m| same_path(&m.path, to));
        match index {
            Some(i) => {
                if state.managed[i].sha256 == hash && on_disk.as_deref() == Some(hash.as_str()) {
                    continue;
                }
                state.managed[i].sha256 = hash.clone();
            }
            None => {
                let mut entry = ManagedFile {
                    path: to.clone(),
                    sha256: hash.clone(),
                    backup: None,
                };
                match on_disk.as_deref() {
                    // The same file is already there (put by hand, or our
                    // copy whose record was lost): adopt it.
                    Some(h) if h == hash => {
                        state.managed.push(entry);
                        save(dir, state)?;
                        continue;
                    }
                    Some(_) => {
                        let backup = format!("{to}.eoa-backup");
                        let saved = root.join(&backup);
                        if saved.exists() {
                            return Err(ClientError::Conflict { path: full });
                        }
                        entry.backup = Some(backup);
                        state.managed.push(entry);
                        save(dir, state)?;
                        std::fs::rename(&full, &saved).map_err(ClientError::io(&full))?;
                    }
                    None => state.managed.push(entry),
                }
            }
        }
        save(dir, state)?;
        copy_atomic(source, &full)?;
        report.added.push(to.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(root: &Path) -> ClientState {
        ClientState {
            format: 1,
            client: "test".into(),
            path: root.to_path_buf(),
            locale: "enUS".into(),
            managed: Vec::new(),
            last_server: None,
        }
    }

    #[test]
    fn adds_backs_up_and_restores() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, root) = (tmp.path().join("state"), tmp.path().join("client"));
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        // The player's own patch-S, and a source patch-V.
        std::fs::write(root.join("Data/patch-S.mpq"), b"players own").unwrap();
        let (v, s) = (tmp.path().join("V.mpq"), tmp.path().join("S.mpq"));
        std::fs::write(&v, b"patch v").unwrap();
        std::fs::write(&s, b"patch s").unwrap();
        let mut st = state(&root);
        let mut report = PrepareReport::default();

        let wanted = vec![(v.clone(), "Data/patch-V.mpq".to_owned())];
        reconcile(&dir, &mut st, &wanted, &mut report).unwrap();
        assert_eq!(
            std::fs::read(root.join("Data/patch-V.mpq")).unwrap(),
            b"patch v"
        );
        // Again: nothing to do.
        let mut again = PrepareReport::default();
        reconcile(&dir, &mut st, &wanted, &mut again).unwrap();
        assert!(again.added.is_empty());

        // Switch to patch-S: patch-V removed, the player's file kept aside.
        let wanted = vec![(s.clone(), "Data/patch-S.mpq".to_owned())];
        reconcile(&dir, &mut st, &wanted, &mut report).unwrap();
        assert!(!root.join("Data/patch-V.mpq").exists());
        assert_eq!(
            std::fs::read(root.join("Data/patch-S.mpq")).unwrap(),
            b"patch s"
        );
        assert_eq!(
            std::fs::read(root.join("Data/patch-S.mpq.eoa-backup")).unwrap(),
            b"players own"
        );

        // Nothing wanted: the player's file is back, no leftovers.
        reconcile(&dir, &mut st, &[], &mut report).unwrap();
        assert_eq!(
            std::fs::read(root.join("Data/patch-S.mpq")).unwrap(),
            b"players own"
        );
        assert!(!root.join("Data/patch-S.mpq.eoa-backup").exists());
        assert!(st.managed.is_empty());
    }

    #[test]
    fn never_deletes_a_tracked_file_the_player_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, root) = (tmp.path().join("state"), tmp.path().join("client"));
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let v = tmp.path().join("V.mpq");
        std::fs::write(&v, b"patch v").unwrap();
        let mut st = state(&root);
        let mut report = PrepareReport::default();
        let wanted = vec![(v, "Data/patch-V.mpq".to_owned())];
        reconcile(&dir, &mut st, &wanted, &mut report).unwrap();
        std::fs::write(root.join("Data/patch-V.mpq"), b"edited").unwrap();
        reconcile(&dir, &mut st, &[], &mut report).unwrap();
        assert_eq!(
            std::fs::read(root.join("Data/patch-V.mpq")).unwrap(),
            b"edited"
        );
        assert_eq!(report.warnings.len(), 1);
    }
}

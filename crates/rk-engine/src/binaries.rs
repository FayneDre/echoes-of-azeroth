//! Programs and libraries an extraction put in place, listed so that a later
//! check finds the ones removed since (an antivirus quarantine is the usual
//! cause, ADR 0020). Only `.exe` and `.dll` files are listed: they are what
//! antivirus tools take away, and checking them is cheap.

use std::path::{Path, PathBuf};

/// The list, written next to the extracted files.
pub const LIST: &str = ".rk-binaries";

/// Whether a file name is a program or a library.
pub fn is_binary(name: &Path) -> bool {
    name.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
}

/// Writes the list of `files` (relative to `dir`, with their sizes); nothing
/// when none is a binary.
pub(crate) fn write_list(dir: &Path, files: &[(PathBuf, u64)]) -> std::io::Result<()> {
    let mut text = String::new();
    for (relative, size) in files.iter().filter(|(p, _)| is_binary(p)) {
        let name = relative.to_string_lossy().replace('\\', "/");
        text.push_str(&format!("{name}\t{size}\n"));
    }
    if text.is_empty() {
        return Ok(());
    }
    std::fs::write(dir.join(LIST), text)
}

/// Listed binaries of `dir` that are gone or changed size, relative to
/// `dir`; `None` when `dir` has no list (extracted by an older launcher).
pub fn missing(dir: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(dir.join(LIST)).ok()?;
    let mut missing = Vec::new();
    for line in text.lines() {
        let Some((name, size)) = line.split_once('\t') else {
            continue;
        };
        let Ok(size) = size.trim().parse::<u64>() else {
            continue;
        };
        let path = dir.join(name);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() == size => {}
            _ => missing.push(name.to_owned()),
        }
    }
    Some(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removed_or_emptied_binary_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.exe"), b"1234").unwrap();
        std::fs::create_dir(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("lib/b.DLL"), b"12").unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"x").unwrap();
        assert_eq!(missing(dir.path()), None, "no list yet");
        write_list(
            dir.path(),
            &[
                ("a.exe".into(), 4),
                (Path::new("lib").join("b.DLL"), 2),
                ("readme.txt".into(), 1),
            ],
        )
        .unwrap();
        assert_eq!(missing(dir.path()), Some(vec![]));
        std::fs::remove_file(dir.path().join("lib/b.DLL")).unwrap();
        std::fs::write(dir.path().join("a.exe"), b"").unwrap();
        assert_eq!(
            missing(dir.path()),
            Some(vec!["a.exe".to_owned(), "lib/b.DLL".to_owned()])
        );
    }
}

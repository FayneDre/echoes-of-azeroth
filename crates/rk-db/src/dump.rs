//! Reading the dumps made by [`Instance::backup`](crate::Instance::backup).
//!
//! A dump may come from someone else (an imported realm, ADR 0020), so it
//! is never handed to the `mysql` client as it is: [`extract_database`]
//! keeps the statements of one database, without `DEFINER` clauses, and
//! the instance replays them as the realm's service account with the
//! client's own commands turned off.

use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::DbError;

/// Start of the last line `mysqldump` writes once a dump is complete.
const COMPLETED: &[u8] = b"-- Dump completed";

/// Databases a dump holds, in order: the names of its `USE` lines
/// (`mysqldump --databases` writes one before each database). Streams the
/// file, whatever its size.
pub fn dump_databases(path: &Path) -> Result<Vec<String>, DbError> {
    let mut out = Vec::new();
    for_each_line(path, |line| {
        if let Some(name) = use_line(line) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
        Ok(())
    })?;
    Ok(out)
}

/// Whether `mysqldump` finished the dump: its last line is
/// `-- Dump completed on <date>` (a dump cut by a crash, a full disk or a
/// truncated download has none).
pub fn dump_complete(path: &Path) -> Result<bool, DbError> {
    let mut file = std::fs::File::open(path).map_err(DbError::io(path))?;
    let len = file.metadata().map_err(DbError::io(path))?.len();
    let start = len.saturating_sub(4096);
    file.seek(SeekFrom::Start(start))
        .map_err(DbError::io(path))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).map_err(DbError::io(path))?;
    Ok(tail
        .split(|&b| b == b'\n')
        .map(<[u8]>::trim_ascii)
        .rfind(|line| !line.is_empty())
        .is_some_and(|line| line.starts_with(COMPLETED)))
}

/// Writes to `out` the statements of database `name` in `dump`: the
/// session settings before the first database (character set, time zone,
/// checks), then every line while `name` is the current database. `USE`
/// and `CREATE DATABASE` lines are left out (the caller creates the
/// database and connects to it), and so are `DEFINER` clauses: triggers,
/// views and routines then belong to the account that replays them.
/// Returns whether the dump holds `name` at all.
pub(crate) fn extract_database(dump: &Path, name: &str, out: &Path) -> Result<bool, DbError> {
    let file = std::fs::File::create(out).map_err(DbError::io(out))?;
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    let mut current: Option<String> = None;
    let mut found = false;
    for_each_line(dump, |line| {
        if let Some(db) = use_line(line) {
            found |= db == name;
            current = Some(db);
            return Ok(());
        }
        let wanted = match &current {
            None => true, // the header: session settings
            Some(db) => db == name,
        };
        if !wanted || line.starts_with(b"CREATE DATABASE") {
            return Ok(());
        }
        let line = if line.starts_with(b"INSERT INTO") {
            std::borrow::Cow::Borrowed(line)
        } else {
            strip_definers(line)
        };
        writer.write_all(&line).map_err(DbError::io(out))
    })?;
    writer.flush().map_err(DbError::io(out))?;
    Ok(found)
}

/// Calls `f` with each line of `path` (its `\n` included), streaming.
fn for_each_line(
    path: &Path,
    mut f: impl FnMut(&[u8]) -> Result<(), DbError>,
) -> Result<(), DbError> {
    let file = std::fs::File::open(path).map_err(DbError::io(path))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(DbError::io(path))?
            == 0
        {
            return Ok(());
        }
        f(&line)?;
    }
}

/// `USE `name`;` → `name`.
fn use_line(line: &[u8]) -> Option<String> {
    let rest = line.strip_prefix(b"USE `")?;
    let end = rest.iter().position(|&b| b == b'`')?;
    let tail = &rest[end + 1..];
    if !tail.trim_ascii().eq(b";") {
        return None;
    }
    std::str::from_utf8(&rest[..end]).ok().map(str::to_owned)
}

/// Removes every `DEFINER=<user>@<host>` (each part quoted with backticks
/// or single quotes) from a line. Another form (`DEFINER=CURRENT_USER`) is
/// left: it names the account that replays the dump anyway, and the
/// server refuses any other definer to that account.
fn strip_definers(line: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    const KEY: &[u8] = b"DEFINER=";
    if !line.windows(KEY.len()).any(|w| w == KEY) {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i..].starts_with(KEY) {
            let after = i + KEY.len();
            if let Some(end) = quoted(line, after)
                .filter(|&at| line.get(at) == Some(&b'@'))
                .and_then(|at| quoted(line, at + 1))
            {
                i = end;
                continue;
            }
        }
        out.push(line[i]);
        i += 1;
    }
    std::borrow::Cow::Owned(out)
}

/// End (exclusive) of a backtick- or single-quoted name starting at `at`.
fn quoted(line: &[u8], at: usize) -> Option<usize> {
    let quote = *line.get(at)?;
    if quote != b'`' && quote != b'\'' {
        return None;
    }
    let close = line[at + 1..].iter().position(|&b| b == quote)?;
    Some(at + 1 + close + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_databases_of_a_dump() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dump.sql");
        std::fs::write(
            &path,
            "-- MySQL dump\n\
             CREATE DATABASE /*!32312 IF NOT EXISTS*/ `acore_auth`;\n\
             USE `acore_auth`;\n\
             INSERT INTO `t` VALUES ('USE `fake`;');\n\
             USE `acore_characters`;\r\n\
             USE `acore_auth`;\n",
        )
        .unwrap();
        assert_eq!(
            dump_databases(&path).unwrap(),
            ["acore_auth", "acore_characters"]
        );
    }

    #[test]
    fn a_dump_is_complete_only_with_its_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dump.sql");
        std::fs::write(
            &path,
            "USE `a`;\n\n-- Dump completed on 2026-11-18 21:24:54\n\n",
        )
        .unwrap();
        assert!(dump_complete(&path).unwrap());
        std::fs::write(&path, "USE `a`;\nINSERT INTO `t` VALUES (1),\n").unwrap();
        assert!(!dump_complete(&path).unwrap());
        std::fs::write(&path, "").unwrap();
        assert!(!dump_complete(&path).unwrap());
    }

    #[test]
    fn extracts_one_database_with_the_header_and_without_definers() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("dump.sql");
        std::fs::write(
            &dump,
            "/*!40101 SET NAMES utf8mb4 */;\n\
             CREATE DATABASE `acore_auth`;\n\
             USE `acore_auth`;\n\
             INSERT INTO `account` VALUES (1);\n\
             CREATE DATABASE `acore_characters`;\n\
             USE `acore_characters`;\n\
             INSERT INTO `characters` VALUES ('DEFINER=`x`@`y`');\n\
             /*!50017 DEFINER=`root`@`localhost`*/ /*!50003 TRIGGER `t` */;\n\
             CREATE DEFINER='root'@'%' PROCEDURE `p`() BEGIN END;\n\
             USE `acore_auth`;\n\
             INSERT INTO `account` VALUES (2);\n",
        )
        .unwrap();
        let out = dir.path().join("chars.sql");
        assert!(extract_database(&dump, "acore_characters", &out).unwrap());
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "/*!40101 SET NAMES utf8mb4 */;\n\
             INSERT INTO `characters` VALUES ('DEFINER=`x`@`y`');\n\
             /*!50017 */ /*!50003 TRIGGER `t` */;\n\
             CREATE  PROCEDURE `p`() BEGIN END;\n"
        );
        assert!(extract_database(&dump, "acore_auth", &out).unwrap());
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "/*!40101 SET NAMES utf8mb4 */;\n\
             INSERT INTO `account` VALUES (1);\n\
             INSERT INTO `account` VALUES (2);\n"
        );
        assert!(!extract_database(&dump, "acore_playerbots", &out).unwrap());
    }

    #[test]
    fn definers_in_other_forms_are_left() {
        assert_eq!(
            &*strip_definers(b"CREATE DEFINER=CURRENT_USER VIEW v"),
            b"CREATE DEFINER=CURRENT_USER VIEW v"
        );
        assert_eq!(
            &*strip_definers(b"DEFINER=`a`@`b` DEFINER=`unclosed"),
            b" DEFINER=`unclosed"
        );
    }
}

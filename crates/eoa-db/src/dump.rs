//! Reading the dumps made by [`Instance::backup`](crate::Instance::backup).

use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::DbError;

/// Databases a dump holds, in order: the names of its `USE` lines
/// (`mysqldump --databases` writes one before each database). Streams the
/// file, whatever its size.
pub fn dump_databases(path: &Path) -> Result<Vec<String>, DbError> {
    let file = std::fs::File::open(path).map_err(DbError::io(path))?;
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut line = Vec::new();
    let mut out = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(DbError::io(path))?
            == 0
        {
            return Ok(out);
        }
        if let Some(name) = use_line(&line) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
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
}

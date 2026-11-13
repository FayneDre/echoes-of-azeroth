use eoa_core::manifest::ConfigValue;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfError {
    #[error("invalid key '{0}': keys are made of letters, digits, '.', '_' and '-'")]
    InvalidKey(String),
    #[error("invalid value for '{key}': strings cannot contain double quotes or line breaks")]
    InvalidValue { key: String },
}

#[derive(Debug, Clone, PartialEq)]
enum Line {
    /// `Key = value` (original text kept for lossless output when untouched).
    Entry {
        key: String,
        raw_value: String,
        text: String,
    },
    /// Comment, blank line or anything we do not interpret.
    Other(String),
}

/// A parsed `.conf` file. Rendering an unmodified file returns the input unchanged
/// (apart from line endings, normalised to `\n`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfFile {
    lines: Vec<Line>,
}

impl ConfFile {
    pub fn parse(text: &str) -> Self {
        let lines = text
            .lines()
            .map(|line| match parse_entry(line) {
                Some((key, raw_value)) => Line::Entry {
                    key,
                    raw_value,
                    text: line.to_owned(),
                },
                None => Line::Other(line.to_owned()),
            })
            .collect();
        Self { lines }
    }

    /// Keys in file order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().filter_map(|line| match line {
            Line::Entry { key, .. } => Some(key.as_str()),
            Line::Other(_) => None,
        })
    }

    /// Raw value as written (quotes included), if the key is present.
    /// With duplicates, this is the first occurrence: the one the servers use.
    pub fn raw(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|line| match line {
            Line::Entry {
                key: k, raw_value, ..
            } if k == key => Some(raw_value.as_str()),
            _ => None,
        })
    }

    /// Value with surrounding quotes removed.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.raw(key).map(unquote)
    }

    /// Sets `key`, replacing the existing line in place (comments around it are
    /// kept) or appending it under a managed section at the end of the file.
    pub fn set(&mut self, key: &str, value: &ConfigValue) -> Result<(), ConfError> {
        validate_key(key)?;
        let raw_value = format_value(key, value)?;
        let text = format!("{key} = {raw_value}");
        // AzerothCore keeps the *first* occurrence of a duplicated key and
        // ignores the others (Config.cpp, IsDuplicateOption): update that one.
        if let Some(line) = self
            .lines
            .iter_mut()
            .find(|line| matches!(line, Line::Entry { key: k, .. } if k == key))
        {
            *line = Line::Entry {
                key: key.to_owned(),
                raw_value,
                text,
            };
            return Ok(());
        }
        if !self
            .lines
            .iter()
            .any(|line| matches!(line, Line::Other(t) if t == MANAGED_HEADER))
        {
            self.lines.push(Line::Other(String::new()));
            self.lines.push(Line::Other(MANAGED_HEADER.to_owned()));
        }
        self.lines.push(Line::Entry {
            key: key.to_owned(),
            raw_value,
            text,
        });
        Ok(())
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Entry { text, .. } | Line::Other(text) => out.push_str(text),
            }
            out.push('\n');
        }
        out
    }
}

/// Header of the section holding keys absent from the upstream file.
const MANAGED_HEADER: &str =
    "# --- Added by Echoes of Azeroth (keys not present in the upstream .conf.dist) ---";

fn parse_entry(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.is_empty() {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    let key = key.trim();
    if validate_key(key).is_err() {
        return None;
    }
    Some((key.to_owned(), value.trim().to_owned()))
}

fn validate_key(key: &str) -> Result<(), ConfError> {
    let valid = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(ConfError::InvalidKey(key.to_owned()))
    }
}

fn format_value(key: &str, value: &ConfigValue) -> Result<String, ConfError> {
    match value {
        ConfigValue::Integer(v) => Ok(v.to_string()),
        ConfigValue::Number(v) => Ok(v.to_string()),
        ConfigValue::Text(v) if v.contains(['"', '\n', '\r']) => Err(ConfError::InvalidValue {
            key: key.to_owned(),
        }),
        ConfigValue::Text(v) => Ok(format!("\"{v}\"")),
    }
}

fn unquote(raw: &str) -> &str {
    raw.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(raw)
}

/// Canonical form used to compare values regardless of quoting style.
pub(crate) fn canonical(raw: &str) -> &str {
    unquote(raw.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
###############
# Header comment
#
#    DataDir
#        Description: Data directory.
DataDir = \".\"

Updates.EnableDatabases = 7
  Indented.Key=1
# Disabled.Key = 5
";

    #[test]
    fn parsing_is_lossless() {
        assert_eq!(ConfFile::parse(SAMPLE).render(), SAMPLE);
    }

    #[test]
    fn reads_values_and_ignores_comments() {
        let file = ConfFile::parse(SAMPLE);
        assert_eq!(file.get("DataDir"), Some("."));
        assert_eq!(file.raw("DataDir"), Some("\".\""));
        assert_eq!(file.get("Indented.Key"), Some("1"));
        assert_eq!(file.get("Disabled.Key"), None);
        assert_eq!(
            file.keys().collect::<Vec<_>>(),
            vec!["DataDir", "Updates.EnableDatabases", "Indented.Key"]
        );
    }

    #[test]
    fn set_replaces_in_place_and_keeps_comments() {
        let mut file = ConfFile::parse(SAMPLE);
        file.set("Updates.EnableDatabases", &ConfigValue::Integer(0))
            .unwrap();
        file.set("DataDir", &ConfigValue::Text("C:/data".into()))
            .unwrap();
        let out = file.render();
        assert!(out.contains("Updates.EnableDatabases = 0\n"));
        assert!(out.contains("DataDir = \"C:/data\"\n"));
        assert!(out.contains("#        Description: Data directory.\n"));
        assert!(!out.contains("Added by Echoes of Azeroth"));
    }

    #[test]
    fn set_appends_unknown_keys_under_a_managed_section_once() {
        let mut file = ConfFile::parse(SAMPLE);
        file.set("New.Key", &ConfigValue::Integer(1)).unwrap();
        file.set("Other.Key", &ConfigValue::Text("x".into()))
            .unwrap();
        let out = file.render();
        assert_eq!(out.matches("Added by Echoes of Azeroth").count(), 1);
        assert!(out.ends_with("New.Key = 1\nOther.Key = \"x\"\n"));
    }

    #[test]
    fn duplicate_keys_follow_the_server_first_occurrence_wins() {
        let mut file = ConfFile::parse("A = 1\nA = 2\n");
        assert_eq!(file.get("A"), Some("1"));
        file.set("A", &ConfigValue::Integer(3)).unwrap();
        assert_eq!(file.render(), "A = 3\nA = 2\n");
    }

    #[test]
    fn rejects_unsafe_values_and_keys() {
        let mut file = ConfFile::default();
        assert!(file.set("Bad Key", &ConfigValue::Integer(1)).is_err());
        assert!(file.set("K", &ConfigValue::Text("a\"b".into())).is_err());
        assert!(file.set("K", &ConfigValue::Text("a\nb".into())).is_err());
    }
}

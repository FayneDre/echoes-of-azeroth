//! `${name}` placeholders in manifest values (ADR 0007).
//!
//! A value that is exactly one placeholder takes the variable's type (a port
//! stays a number); otherwise placeholders are interpolated into the text.
//! `$` not followed by `{` is literal.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::manifest::ConfigValue;

/// Placeholders allowed in `instance.config`, besides `port.<name>`.
pub const INSTANCE_VARIABLES: &[&str] = &[
    "dir.instance",
    "dir.logs",
    "dir.sql",
    "dir.data",
    "tool.mysql",
    "network.bind",
    "modules.dbimport",
];

/// Placeholders allowed in `instance.connection`.
pub const CONNECTION_VARIABLES: &[&str] = &["db.port", "db.user", "db.password", "db.name"];

/// Variable name -> value.
pub type Variables = BTreeMap<String, ConfigValue>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TemplateError {
    #[error("unterminated placeholder in '{0}'")]
    Unterminated(String),
    #[error("unknown placeholder '${{{0}}}'")]
    Unknown(String),
}

/// Names of the placeholders in `text`, in order.
pub fn placeholders(text: &str) -> Result<Vec<&str>, TemplateError> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| TemplateError::Unterminated(text.to_owned()))?;
        names.push(&after[..end]);
        rest = &after[end + 1..];
    }
    Ok(names)
}

/// Text form of a value inside a larger string (no quotes).
fn raw(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Text(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Interpolates every placeholder of `text`.
pub fn expand_str(text: &str, vars: &Variables) -> Result<String, TemplateError> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| TemplateError::Unterminated(text.to_owned()))?;
        let name = &after[..end];
        let value = vars
            .get(name)
            .ok_or_else(|| TemplateError::Unknown(name.to_owned()))?;
        out.push_str(&raw(value));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Expands a config value: `"${port.auth}"` becomes the port number,
/// `"${dir.logs}/x"` a string, other values are returned unchanged.
pub fn expand(value: &ConfigValue, vars: &Variables) -> Result<ConfigValue, TemplateError> {
    let ConfigValue::Text(text) = value else {
        return Ok(value.clone());
    };
    if let Some(name) = text.strip_prefix("${").and_then(|t| t.strip_suffix('}')) {
        if !name.contains(['$', '{', '}']) {
            return vars
                .get(name)
                .cloned()
                .ok_or_else(|| TemplateError::Unknown(name.to_owned()));
        }
    }
    expand_str(text, vars).map(ConfigValue::Text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Variables {
        Variables::from([
            ("port.auth".into(), ConfigValue::Integer(3724)),
            ("dir.logs".into(), ConfigValue::Text("C:/i/logs".into())),
        ])
    }

    #[test]
    fn single_placeholder_keeps_the_type() {
        let v = expand(&ConfigValue::Text("${port.auth}".into()), &vars()).unwrap();
        assert_eq!(v, ConfigValue::Integer(3724));
    }

    #[test]
    fn interpolation_and_literals() {
        let v = expand(
            &ConfigValue::Text("${dir.logs}/a $5 ${port.auth}".into()),
            &vars(),
        );
        assert_eq!(v.unwrap(), ConfigValue::Text("C:/i/logs/a $5 3724".into()));
        assert_eq!(
            expand(&ConfigValue::Integer(0), &vars()).unwrap(),
            ConfigValue::Integer(0)
        );
    }

    #[test]
    fn errors() {
        assert_eq!(
            expand_str("${nope}", &vars()),
            Err(TemplateError::Unknown("nope".into()))
        );
        assert!(matches!(
            placeholders("a ${b"),
            Err(TemplateError::Unterminated(_))
        ));
        assert_eq!(placeholders("${a}-${b}").unwrap(), ["a", "b"]);
    }
}

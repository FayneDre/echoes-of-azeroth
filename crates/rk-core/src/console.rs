//! Console commands of the GM panel (ADR 0020): a manifest
//! [`ConsoleAction`] plus the values the player typed become one command
//! line. Every value is checked first, so a value can never add a second
//! command, a line break or an unbalanced quote.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::manifest::{ConsoleAction, ConsoleParamKind};

/// Longest free text when the manifest sets none.
const TEXT_LIMIT: u32 = 255;
/// Longest place name.
const LOCATION_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConsoleError {
    #[error("missing value for '{0}'")]
    Missing(String),
    #[error("'{0}' must be a character name (2 to 12 letters)")]
    Character(String),
    #[error("'{param}' must be a whole number from {min} to {max}")]
    Number { param: String, min: i64, max: i64 },
    #[error("'{param}' must be one line of at most {max} characters, without double quotes")]
    Text { param: String, max: u32 },
    #[error("'{0}' is not one of the offered values")]
    Choice(String),
    #[error("the command uses '{{{0}}}', which is not a parameter")]
    UnknownPlaceholder(String),
}

/// The command line for `action` with `values` (by parameter id); a number
/// left empty takes its default.
pub fn render(
    action: &ConsoleAction,
    values: &BTreeMap<String, String>,
) -> Result<String, ConsoleError> {
    let mut checked = BTreeMap::new();
    for param in &action.params {
        let raw = values.get(&param.id).map(|v| v.trim()).unwrap_or("");
        let value = match &param.kind {
            ConsoleParamKind::Character => {
                let count = raw.chars().count();
                if !(2..=12).contains(&count) || !raw.chars().all(char::is_alphabetic) {
                    return Err(ConsoleError::Character(param.id.clone()));
                }
                raw.to_owned()
            }
            ConsoleParamKind::Number { min, max, default } => {
                let number = if raw.is_empty() {
                    default.ok_or_else(|| ConsoleError::Missing(param.id.clone()))?
                } else {
                    raw.parse::<i64>().map_err(|_| ConsoleError::Number {
                        param: param.id.clone(),
                        min: *min,
                        max: *max,
                    })?
                };
                if number < *min || number > *max {
                    return Err(ConsoleError::Number {
                        param: param.id.clone(),
                        min: *min,
                        max: *max,
                    });
                }
                number.to_string()
            }
            ConsoleParamKind::Text { max_length } => {
                let max = max_length.unwrap_or(TEXT_LIMIT);
                if raw.is_empty() {
                    return Err(ConsoleError::Missing(param.id.clone()));
                }
                if raw.chars().count() > max as usize || !one_line(raw) {
                    return Err(ConsoleError::Text {
                        param: param.id.clone(),
                        max,
                    });
                }
                raw.to_owned()
            }
            ConsoleParamKind::Location => {
                if raw.is_empty() {
                    return Err(ConsoleError::Missing(param.id.clone()));
                }
                if !valid_location(raw) {
                    return Err(ConsoleError::Text {
                        param: param.id.clone(),
                        max: LOCATION_LIMIT as u32,
                    });
                }
                raw.to_owned()
            }
            ConsoleParamKind::Choice { choices } => {
                if !choices.iter().any(|c| c.value == raw) {
                    return Err(ConsoleError::Choice(param.id.clone()));
                }
                raw.to_owned()
            }
        };
        checked.insert(param.id.as_str(), value);
    }
    fill(&action.command, &checked)
}

/// Whether `name` can be a location value: one word (a space would shift
/// the next argument), one line, short. Drivers list only such places.
pub fn valid_location(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= LOCATION_LIMIT
        && one_line(name)
        && !name.chars().any(char::is_whitespace)
}

/// Placeholders of a command, in order (`{id}`).
pub fn placeholders(command: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = command;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start + 1..].find('}') else {
            break;
        };
        found.push(&rest[start + 1..start + 1 + len]);
        rest = &rest[start + 1 + len + 1..];
    }
    found
}

fn fill(command: &str, values: &BTreeMap<&str, String>) -> Result<String, ConsoleError> {
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start + 1..].find('}') else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let name = &rest[start + 1..start + 1 + len];
        let value = values
            .get(name)
            .ok_or_else(|| ConsoleError::UnknownPlaceholder(name.to_owned()))?;
        out.push_str(value);
        rest = &rest[start + 1 + len + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn one_line(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || c == '"')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ConsoleChoice, ConsoleParam};
    use crate::LocalizedText;

    fn label() -> LocalizedText {
        LocalizedText {
            en: "x".into(),
            fr: None,
        }
    }

    fn param(id: &str, kind: ConsoleParamKind) -> ConsoleParam {
        ConsoleParam {
            id: id.into(),
            label: label(),
            kind,
        }
    }

    fn action(command: &str, params: Vec<ConsoleParam>) -> ConsoleAction {
        ConsoleAction {
            id: "a".into(),
            label: label(),
            description: None,
            command: command.into(),
            params,
        }
    }

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn values_fill_the_command() {
        let send = action(
            r#"send items {character} "{subject}" "{subject}" {item}:{count}"#,
            vec![
                param("character", ConsoleParamKind::Character),
                param(
                    "item",
                    ConsoleParamKind::Number {
                        min: 1,
                        max: 999_999,
                        default: None,
                    },
                ),
                param(
                    "count",
                    ConsoleParamKind::Number {
                        min: 1,
                        max: 1000,
                        default: Some(1),
                    },
                ),
                param(
                    "subject",
                    ConsoleParamKind::Text {
                        max_length: Some(60),
                    },
                ),
            ],
        );
        let line = render(
            &send,
            &values(&[
                ("character", "Élodie"),
                ("item", "6948"),
                ("subject", "Cadeau"),
            ]),
        )
        .unwrap();
        assert_eq!(line, r#"send items Élodie "Cadeau" "Cadeau" 6948:1"#);
    }

    #[test]
    fn values_cannot_break_out_of_the_command() {
        let announce = action(
            "announce {message}",
            vec![param(
                "message",
                ConsoleParamKind::Text { max_length: None },
            )],
        );
        for bad in ["hi\nserver shutdown 1", "say \"x", "", "\u{7}"] {
            assert!(
                render(&announce, &values(&[("message", bad)])).is_err(),
                "{bad:?}"
            );
        }
        let revive = action("revive {c}", vec![param("c", ConsoleParamKind::Character)]);
        for bad in ["A", "Bob; saveall", "Bob2", "ThirteenChars"] {
            assert!(render(&revive, &values(&[("c", bad)])).is_err(), "{bad:?}");
        }
        let tele = action(
            "teleport name {c} {at}",
            vec![param("at", ConsoleParamKind::Location)],
        );
        assert!(render(&tele, &values(&[("at", "Storm wind")])).is_err());
        assert!(valid_location("Stormwind"));
        assert!(!valid_location("Storm wind"));
        assert!(!valid_location(""));
        assert!(!valid_location(&"x".repeat(LOCATION_LIMIT + 1)));
        let level = action(
            "character level {c} {l}",
            vec![param(
                "l",
                ConsoleParamKind::Number {
                    min: 1,
                    max: 80,
                    default: None,
                },
            )],
        );
        assert!(render(&level, &values(&[("l", "81")])).is_err());
        assert!(render(&level, &values(&[("l", "")])).is_err());
    }

    #[test]
    fn choices_and_placeholders_are_checked() {
        let unstuck = action(
            "unstuck {c} {where}",
            vec![
                param("c", ConsoleParamKind::Character),
                param(
                    "where",
                    ConsoleParamKind::Choice {
                        choices: vec![ConsoleChoice {
                            value: "inn".into(),
                            label: label(),
                        }],
                    },
                ),
            ],
        );
        assert_eq!(
            render(&unstuck, &values(&[("c", "Bob"), ("where", "inn")])).unwrap(),
            "unstuck Bob inn"
        );
        assert!(render(&unstuck, &values(&[("c", "Bob"), ("where", "startzone")])).is_err());
        let broken = action("revive {who}", vec![]);
        assert_eq!(
            render(&broken, &values(&[])),
            Err(ConsoleError::UnknownPlaceholder("who".into()))
        );
        assert_eq!(placeholders("a {x} b {y}"), ["x", "y"]);
    }
}

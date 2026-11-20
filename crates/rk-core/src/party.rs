//! A party of bots formed from the launcher (ADR 0025): the console lines of
//! a manifest [`Party`], every value checked first, and the `RKPARTY` reply
//! lines the module answers with.

use thiserror::Error;

use crate::manifest::{Party, PartyClass};

/// Every reply line of the party protocol starts with this word.
pub const PREFIX: &str = "RKPARTY";

/// Highest level the launcher accepts; the server checks its own maximum.
pub const MAX_LEVEL: u32 = 255;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PartyError {
    #[error("'{0}' is not a character name (2 to 12 letters)")]
    Character(String),
    #[error("unknown class '{0}'")]
    Class(String),
    #[error("class '{class}' has no spec {spec}")]
    Spec { class: String, spec: u32 },
    #[error("a {class} bot must be level {min} to {max}")]
    Level { class: String, min: u32, max: u32 },
    /// Refused by the server: a stable code of the module (`full`, ...).
    #[error("the server refused: {0}")]
    Refused(String),
    #[error("the server did not answer the party command")]
    NoAnswer,
}

/// A bot of the player's party, from a `MEMBER` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// Launcher class id (`death-knight`), or the server's number when the
    /// manifest does not list it.
    pub class: String,
    pub level: u32,
    /// Logged in and set up; `false` while it is still joining.
    pub ready: bool,
}

fn character(name: &str) -> Result<&str, PartyError> {
    let name = name.trim();
    let count = name.chars().count();
    if (2..=12).contains(&count) && name.chars().all(char::is_alphabetic) {
        Ok(name)
    } else {
        Err(PartyError::Character(name.to_owned()))
    }
}

fn class<'a>(party: &'a Party, id: &str) -> Result<&'a PartyClass, PartyError> {
    party
        .classes
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| PartyError::Class(id.to_owned()))
}

/// The line that adds a bot of `class` (launcher id) with premade spec
/// `spec` at `level` to `player`'s party.
pub fn add_line(
    party: &Party,
    player: &str,
    class_id: &str,
    spec: u32,
    level: u32,
) -> Result<String, PartyError> {
    let player = character(player)?;
    let class = class(party, class_id)?;
    if !class.specs.iter().any(|s| s.number == spec) {
        return Err(PartyError::Spec {
            class: class.id.clone(),
            spec,
        });
    }
    if !(class.min_level..=MAX_LEVEL).contains(&level) {
        return Err(PartyError::Level {
            class: class.id.clone(),
            min: class.min_level,
            max: MAX_LEVEL,
        });
    }
    Ok(party
        .commands
        .add
        .replace("{player}", player)
        .replace("{class}", &class.token)
        .replace("{spec}", &spec.to_string())
        .replace("{level}", &level.to_string()))
}

/// The line that sends away `player`'s bots.
pub fn remove_line(party: &Party, player: &str) -> Result<String, PartyError> {
    Ok(party
        .commands
        .remove
        .replace("{player}", character(player)?))
}

/// The line that lists `player`'s bots.
pub fn list_line(party: &Party, player: &str) -> Result<String, PartyError> {
    Ok(party.commands.list.replace("{player}", character(player)?))
}

/// The words of a reply line after [`PREFIX`] (console prompts or log
/// headers before it are skipped), or `None` for any other line.
fn words(line: &str) -> Option<Vec<&str>> {
    let at = line.find(PREFIX)?;
    Some(line[at + PREFIX.len()..].split_whitespace().collect())
}

/// Whether `line` ends the answer to a party command: `OK`, `ERROR` or
/// `END`. `READY` and `FAILED` lines come later, on their own.
pub fn ends_answer(line: &str) -> bool {
    matches!(words(line).as_deref(), Some(["OK" | "ERROR" | "END", ..]))
}

fn refused(lines: &[String]) -> Option<PartyError> {
    lines.iter().find_map(|line| match words(line)?.as_slice() {
        ["ERROR", code, ..] => Some(PartyError::Refused((*code).to_owned())),
        _ => None,
    })
}

/// The bot an add command picked.
pub fn added(lines: &[String]) -> Result<String, PartyError> {
    if let Some(error) = refused(lines) {
        return Err(error);
    }
    lines
        .iter()
        .find_map(|line| match words(line)?.as_slice() {
            ["OK", bot, ..] => Some((*bot).to_owned()),
            _ => None,
        })
        .ok_or(PartyError::NoAnswer)
}

/// The bots a remove command sent away.
pub fn removed(lines: &[String]) -> Result<Vec<String>, PartyError> {
    if let Some(error) = refused(lines) {
        return Err(error);
    }
    if !lines.iter().any(|l| ends_answer(l)) {
        return Err(PartyError::NoAnswer);
    }
    Ok(lines
        .iter()
        .filter_map(|line| match words(line)?.as_slice() {
            ["REMOVED", name, ..] => Some((*name).to_owned()),
            _ => None,
        })
        .collect())
}

/// The bots of a list command, with launcher class ids.
pub fn members(party: &Party, lines: &[String]) -> Result<Vec<Member>, PartyError> {
    if let Some(error) = refused(lines) {
        return Err(error);
    }
    if !lines.iter().any(|l| ends_answer(l)) {
        return Err(PartyError::NoAnswer);
    }
    Ok(lines
        .iter()
        .filter_map(|line| match words(line)?.as_slice() {
            ["MEMBER", name, class, level, state, ..] => {
                let number: u8 = class.parse().ok()?;
                Some(Member {
                    name: (*name).to_owned(),
                    class: party
                        .classes
                        .iter()
                        .find(|c| c.class_id == number)
                        .map_or_else(|| number.to_string(), |c| c.id.clone()),
                    level: level.parse().ok()?,
                    ready: *state == "ready",
                })
            }
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PartyCommands, PartyRole, PartySpec};
    use crate::text::LocalizedText;

    fn party() -> Party {
        let name = |en: &str| LocalizedText {
            en: en.into(),
            fr: None,
        };
        Party {
            size: 5,
            commands: PartyCommands {
                add: ".rk party add {player} {class} {spec} {level}".into(),
                remove: ".rk party remove {player}".into(),
                list: ".rk party list {player}".into(),
            },
            classes: vec![
                PartyClass {
                    id: "warrior".into(),
                    class_id: 1,
                    token: "warrior".into(),
                    name: name("Warrior"),
                    min_level: 1,
                    specs: vec![PartySpec {
                        number: 2,
                        role: PartyRole::Tank,
                        name: name("Protection"),
                    }],
                },
                PartyClass {
                    id: "death-knight".into(),
                    class_id: 6,
                    token: "dk".into(),
                    name: name("Death Knight"),
                    min_level: 55,
                    specs: vec![PartySpec {
                        number: 0,
                        role: PartyRole::Tank,
                        name: name("Blood"),
                    }],
                },
            ],
        }
    }

    #[test]
    fn add_line_checks_every_value() {
        let p = party();
        assert_eq!(
            add_line(&p, "Garona", "death-knight", 0, 60).unwrap(),
            ".rk party add Garona dk 0 60"
        );
        assert_eq!(
            add_line(&p, "Ga rona", "warrior", 2, 10),
            Err(PartyError::Character("Ga rona".into()))
        );
        assert_eq!(
            add_line(&p, "Garona", "mage", 0, 10),
            Err(PartyError::Class("mage".into()))
        );
        assert!(matches!(
            add_line(&p, "Garona", "warrior", 0, 10),
            Err(PartyError::Spec { .. })
        ));
        assert!(matches!(
            add_line(&p, "Garona", "death-knight", 0, 54),
            Err(PartyError::Level { min: 55, .. })
        ));
        assert_eq!(
            remove_line(&p, "Garona").unwrap(),
            ".rk party remove Garona"
        );
    }

    #[test]
    fn replies_are_read_after_the_prefix() {
        let p = party();
        let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(ends_answer("AC> RKPARTY OK Bob"));
        assert!(!ends_answer("RKPARTY READY Bob 60 prot pve"));
        assert_eq!(added(&lines(&["AC> RKPARTY OK Bob"])).unwrap(), "Bob");
        assert_eq!(
            added(&lines(&["RKPARTY ERROR full"])),
            Err(PartyError::Refused("full".into()))
        );
        assert_eq!(added(&lines(&[])), Err(PartyError::NoAnswer));
        let list = members(
            &p,
            &lines(&[
                "RKPARTY MEMBER Bob 6 60 ready",
                "RKPARTY READY Ann 10 arms pve",
                "RKPARTY MEMBER Ann 8 10 joining",
                "RKPARTY END 2",
            ]),
        )
        .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(
            (list[0].class.as_str(), list[0].ready),
            ("death-knight", true)
        );
        assert_eq!((list[1].class.as_str(), list[1].ready), ("8", false));
        assert_eq!(
            removed(&lines(&["RKPARTY REMOVED Bob", "RKPARTY END 1"])).unwrap(),
            vec!["Bob"]
        );
    }
}

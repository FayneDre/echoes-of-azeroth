//! Manifest types. Each struct mirrors a JSON Schema in `catalog/schema/`.
//! Keep both in sync: a schema change without the matching change here (or
//! the reverse) is a bug. `deny_unknown_fields` makes drift fail loudly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::text::LocalizedText;

/// A value written to a `.conf` file. Booleans are `0`/`1`, as upstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
    Integer(i64),
    Number(f64),
    Text(String),
}

impl std::fmt::Display for ConfigValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigValue::Integer(v) => write!(f, "{v}"),
            ConfigValue::Number(v) => write!(f, "{v}"),
            ConfigValue::Text(v) => write!(f, "\"{v}\""),
        }
    }
}

/// Config key -> value.
pub type ConfigMap = BTreeMap<String, ConfigValue>;
/// Config file (relative to the instance `conf` dir) -> keys.
pub type ConfigChanges = BTreeMap<String, ConfigMap>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSource {
    pub repo: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbConnection {
    pub file: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Database {
    pub id: String,
    pub name: String,
    /// Player data is never regenerated, only migrated and backed up.
    pub player_data: bool,
    pub connections: Vec<DbConnection>,
}

// ---------------------------------------------------------------- emulator

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmulatorManifest {
    #[serde(rename = "$schema", default, skip_serializing)]
    pub json_schema: Option<String>,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: LocalizedText,
    pub homepage: String,
    pub license: String,
    pub driver: String,
    pub client: String,
    pub components: Vec<ComponentRef>,
    pub default_flavor: String,
    pub databases: Vec<Database>,
    /// Values imposed in every instance (ADR 0007).
    pub instance: InstanceSpec,
    pub ports: BTreeMap<String, u16>,
    pub processes: Vec<ProcessSpec>,
    /// Realm settings shown in the launcher (rates, realm type...), by
    /// section. Values live in `instance.json` under [`REALM_OPTIONS`].
    #[serde(default)]
    pub settings: Vec<SettingsSection>,
    /// Ready-made console commands of the GM panel (ADR 0020).
    #[serde(default)]
    pub console_actions: Vec<ConsoleAction>,
    /// How to make the running server write its players to the database
    /// before a backup taken while it runs (ADR 0022). Without it, such a
    /// backup holds what the server last saved by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_players: Option<SavePlayers>,
}

/// A console command that saves every player, and the output line that
/// says it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavePlayers {
    /// Sent to the process that takes console commands.
    pub command: String,
    /// Text of the output line printed once the saves are queued.
    pub done: String,
    /// Longest wait for `done` before the backup goes ahead anyway.
    #[serde(default = "default_save_timeout")]
    pub timeout_seconds: u64,
    /// Extra wait after `done`, for writes the server makes in the
    /// background.
    #[serde(default)]
    pub settle_seconds: u64,
}

fn default_save_timeout() -> u64 {
    30
}

/// A console command with parameters the player fills in (GM panel). Sent
/// to the process that takes console commands, once rendered by
/// [`crate::console::render`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsoleAction {
    pub id: String,
    pub label: LocalizedText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LocalizedText>,
    /// The command, with `{param}` placeholders.
    pub command: String,
    #[serde(default)]
    pub params: Vec<ConsoleParam>,
}

/// `deny_unknown_fields` does not work with `flatten`: the JSON Schema
/// rejects unknown fields of a parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleParam {
    pub id: String,
    pub label: LocalizedText,
    #[serde(flatten)]
    pub kind: ConsoleParamKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ConsoleParamKind {
    /// A character name (offered from the characters online).
    Character,
    /// Free text on one line, without double quotes.
    #[serde(rename_all = "camelCase")]
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
    },
    /// A whole number.
    Number {
        min: i64,
        max: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
    },
    /// A named place (offered from the driver's teleport locations).
    Location,
    /// One of fixed values.
    Choice { choices: Vec<ConsoleChoice> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleChoice {
    pub value: String,
    pub label: LocalizedText,
}

/// Key of the realm settings in an instance's option values (module ids
/// may not use it).
pub const REALM_OPTIONS: &str = "realm";

/// A group of realm settings (one panel in the UI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSection {
    pub id: String,
    pub label: LocalizedText,
    pub options: Vec<ModuleOption>,
}

/// Launcher-imposed instance configuration, with `${...}` placeholders
/// (see [`crate::template`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceSpec {
    /// Approximate disk use of one installed instance (server files, SQL
    /// tree, databases), on top of the downloads: free-space check.
    #[serde(rename = "diskSize", default, skip_serializing_if = "Option::is_none")]
    pub disk_size: Option<u64>,
    /// Template of every database connection value (`${db.*}` placeholders).
    pub connection: String,
    /// Config file -> key -> value (Launcher layer, locked).
    pub config: ConfigChanges,
    pub db_import: ToolSpec,
}

/// A command-line tool shipped with the server build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolSpec {
    /// Relative to the instance root.
    pub executable: String,
    pub config_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSpec {
    pub id: String,
    pub executable: String,
    pub config_file: String,
    pub readiness: Readiness,
    /// Command-line arguments; the working directory is the instance root.
    #[serde(default)]
    pub args: Vec<String>,
    pub stop: StopMethod,
    /// Log lines that mark how far the start went (start progress bar,
    /// ADR 0020), in order; lines that never come are skipped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub startup: Vec<StartupMilestone>,
}

/// A start phase: when an output line contains `match`, the start is
/// `progress` (0 to 1) of the way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupMilestone {
    #[serde(rename = "match")]
    pub pattern: String,
    pub progress: f32,
    pub label: LocalizedText,
}

impl Eq for StartupMilestone {}

/// How a server process is stopped cleanly (ADR 0011).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StopMethod {
    /// Close stdin: the process saves and exits at end of input. This also
    /// happens when the launcher dies, since the pipe closes with it.
    #[serde(rename_all = "camelCase")]
    CloseStdin {
        /// Time the shutdown may print nothing before the process is killed
        /// (default 300 s); a shutdown that still logs is never killed.
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    /// Kill: only for processes that keep no unsaved state. The process is
    /// killed with the launcher too (Job Object).
    Terminate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Readiness {
    /// Ready when the named port accepts TCP connections.
    Tcp { port: String },
    /// Ready when a stdout line matches the regular expression.
    LogLine { pattern: String },
}

// ------------------------------------------------------------------ flavor

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlavorManifest {
    #[serde(rename = "$schema", default, skip_serializing)]
    pub json_schema: Option<String>,
    pub schema_version: u32,
    pub id: String,
    pub emulator: String,
    pub name: LocalizedText,
    pub description: LocalizedText,
    pub source: GitSource,
    pub required_modules: Vec<String>,
}

// ------------------------------------------------------------------ module

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModuleManifest {
    #[serde(rename = "$schema", default, skip_serializing)]
    pub json_schema: Option<String>,
    pub schema_version: u32,
    pub id: String,
    pub emulator: String,
    pub name: LocalizedText,
    pub description: LocalizedText,
    pub category: ModuleCategory,
    pub homepage: String,
    pub license: String,
    pub source: GitSource,
    pub compatible_flavors: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    pub removable: bool,
    pub activation: Activation,
    pub database: ModuleDatabase,
    pub config: ModuleConfig,
    #[serde(default)]
    pub options: Vec<ModuleOption>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub post_install: Vec<PostInstallStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleCategory {
    Bots,
    Economy,
    Progression,
    Gameplay,
    QualityOfLife,
    Cosmetic,
    Administration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Activation {
    /// Active whenever compiled in. Only valid for non-removable modules.
    Always,
    /// Toggled through config keys; CI proves `disable` makes the module inert.
    Config {
        enable: ConfigChanges,
        disable: ConfigChanges,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModuleDatabase {
    /// Name used in dbimport `Updates.AllowedModules`.
    pub dbimport_id: String,
    /// SQL applied even when the module is not chosen: it reads its own
    /// tables while disabled (ADR 0008).
    #[serde(default)]
    pub apply_when_disabled: bool,
    pub touches: Vec<String>,
    #[serde(default)]
    pub extra_databases: Vec<Database>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModuleConfig {
    pub files: Vec<ModuleConfigFile>,
    #[serde(default)]
    pub core_overrides: ConfigChanges,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleConfigFile {
    pub file: String,
    pub dist: String,
    #[serde(default)]
    pub defaults: ConfigMap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigTarget {
    pub file: String,
    pub key: String,
}

/// A simplified setting shown in the launcher UI. Applied after activation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ModuleOption {
    Boolean {
        id: String,
        label: LocalizedText,
        #[serde(default)]
        help: Option<LocalizedText>,
        target: ConfigTarget,
        default: bool,
    },
    Integer {
        id: String,
        label: LocalizedText,
        #[serde(default)]
        help: Option<LocalizedText>,
        target: ConfigTarget,
        default: i64,
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
    },
    /// A decimal value (rates: `1` = upstream speed).
    Number {
        id: String,
        label: LocalizedText,
        #[serde(default)]
        help: Option<LocalizedText>,
        target: ConfigTarget,
        default: f64,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
    Choice {
        id: String,
        label: LocalizedText,
        #[serde(default)]
        help: Option<LocalizedText>,
        default: String,
        choices: Vec<OptionChoice>,
    },
}

impl ModuleOption {
    pub fn id(&self) -> &str {
        match self {
            ModuleOption::Boolean { id, .. }
            | ModuleOption::Integer { id, .. }
            | ModuleOption::Number { id, .. }
            | ModuleOption::Choice { id, .. } => id,
        }
    }

    pub fn default_value(&self) -> OptionValue {
        match self {
            ModuleOption::Boolean { default, .. } => OptionValue::Bool(*default),
            ModuleOption::Integer { default, .. } => OptionValue::Integer(*default),
            ModuleOption::Number { default, .. } => OptionValue::Number(*default),
            ModuleOption::Choice { default, .. } => OptionValue::Choice(default.clone()),
        }
    }

    /// Parses a value typed by the player (`true`/`false`/`1`/`0`, a number
    /// in range, or a choice id) and checks it.
    pub fn parse_value(&self, text: &str) -> Result<OptionValue, String> {
        match self {
            ModuleOption::Boolean { .. } => match text {
                "true" | "1" | "on" => Ok(OptionValue::Bool(true)),
                "false" | "0" | "off" => Ok(OptionValue::Bool(false)),
                _ => Err(format!("'{text}' is not true or false")),
            },
            ModuleOption::Integer { min, max, .. } => {
                let n: i64 = text
                    .parse()
                    .map_err(|_| format!("'{text}' is not a whole number"))?;
                if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                    return Err(format!(
                        "{n} is out of range ({} to {})",
                        min.map_or("-".into(), |m| m.to_string()),
                        max.map_or("-".into(), |m| m.to_string())
                    ));
                }
                Ok(OptionValue::Integer(n))
            }
            ModuleOption::Number { min, max, .. } => {
                let n: f64 = text
                    .trim()
                    .replace(',', ".")
                    .parse()
                    .ok()
                    .filter(|n: &f64| n.is_finite())
                    .ok_or_else(|| format!("'{text}' is not a number"))?;
                if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                    return Err(format!(
                        "{n} is out of range ({} to {})",
                        min.map_or("-".into(), |m| m.to_string()),
                        max.map_or("-".into(), |m| m.to_string())
                    ));
                }
                Ok(OptionValue::Number(n))
            }
            ModuleOption::Choice { choices, .. } => {
                if choices.iter().any(|c| c.id == text) {
                    Ok(OptionValue::Choice(text.to_owned()))
                } else {
                    let ids: Vec<&str> = choices.iter().map(|c| c.id.as_str()).collect();
                    Err(format!("'{text}' is not one of: {}", ids.join(", ")))
                }
            }
        }
    }

    /// The value in force: the chosen one when it fits this option, else
    /// the default (an option whose type or choices changed upstream).
    pub fn effective(&self, chosen: Option<&OptionValue>) -> OptionValue {
        match chosen.and_then(|v| self.fit(v)) {
            Some(v) => v,
            None => self.default_value(),
        }
    }

    /// `value` as a value of this option, if it is one (a whole number is
    /// also a decimal one).
    pub fn fit(&self, value: &OptionValue) -> Option<OptionValue> {
        match (self, value) {
            (ModuleOption::Boolean { .. }, OptionValue::Bool(_)) => Some(value.clone()),
            (ModuleOption::Integer { min, max, .. }, OptionValue::Integer(n)) => {
                (!(min.is_some_and(|m| *n < m) || max.is_some_and(|m| *n > m)))
                    .then(|| value.clone())
            }
            (
                ModuleOption::Number { min, max, .. },
                OptionValue::Number(_) | OptionValue::Integer(_),
            ) => {
                let n = match value {
                    OptionValue::Number(n) => *n,
                    OptionValue::Integer(n) => *n as f64,
                    _ => unreachable!(),
                };
                (n.is_finite() && !(min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m)))
                    .then_some(OptionValue::Number(n))
            }
            (ModuleOption::Choice { choices, .. }, OptionValue::Choice(c)) => {
                choices.iter().any(|x| &x.id == c).then(|| value.clone())
            }
            _ => None,
        }
    }
}

/// A module option value chosen by the player (`instance.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionValue {
    Bool(bool),
    Integer(i64),
    Number(f64),
    Choice(String),
}

impl ModuleManifest {
    /// Assets selected by the effective option values, in manifest order.
    pub fn selected_assets(
        &self,
        values: Option<&std::collections::BTreeMap<String, OptionValue>>,
    ) -> Vec<&Asset> {
        let mut ids: Vec<&str> = Vec::new();
        for option in &self.options {
            if let ModuleOption::Choice { choices, .. } = option {
                let value = option.effective(values.and_then(|v| v.get(option.id())));
                if let OptionValue::Choice(chosen) = value {
                    if let Some(choice) = choices.iter().find(|c| c.id == chosen) {
                        ids.extend(choice.assets.iter().map(String::as_str));
                    }
                }
            }
        }
        self.assets
            .iter()
            .filter(|a| ids.contains(&a.id.as_str()))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionChoice {
    pub id: String,
    pub label: LocalizedText,
    #[serde(default)]
    pub help: Option<LocalizedText>,
    #[serde(default)]
    pub assets: Vec<String>,
    #[serde(default)]
    pub config: ConfigChanges,
}

/// Optional files a module ships in its own repository (client MPQ patches,
/// server DBC overrides). Downloaded from the module's pinned commit and
/// verified by sha256, never re-hosted (like server data, ADR 0006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub source: AssetSource,
    /// Archive members copied into the client patch directory (tracked,
    /// removed again when the choice changes).
    #[serde(default)]
    pub client: Vec<AssetFile>,
    /// Archive members overlaid on the instance's server data directory.
    #[serde(default)]
    pub server: Vec<AssetFile>,
    /// Archive folders installed as client add-ons (`from` = a folder of
    /// the archive, `to` = the add-on's folder name), ADR 0020.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addons: Vec<AssetFile>,
}

/// An archive (`.zip` or `.7z`): a file of the module repository at its
/// pinned commit (`path`), or the archive GitHub makes of another
/// repository at a pinned commit (`repository` + `commit`, a `.zip`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetSource {
    /// Path in the module repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Another GitHub repository (`https://github.com/<owner>/<repo>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Full commit of `repository`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub size: u64,
    pub sha256: String,
}

impl AssetSource {
    /// Download URL; `module` is the repository of the module declaring it.
    pub fn url(&self, module: &GitSource) -> Option<String> {
        match (&self.path, &self.repository, &self.commit) {
            (Some(path), None, None) => module.raw_url(path),
            (None, Some(repo), Some(commit)) => {
                let repo = repo.strip_prefix("https://github.com/")?;
                let repo = repo.trim_end_matches('/').trim_end_matches(".git");
                Some(format!("https://github.com/{repo}/archive/{commit}.zip"))
            }
            _ => None,
        }
    }

    /// Whether the archive is a 7-Zip one (else a ZIP).
    pub fn is_seven_zip(&self) -> bool {
        self.path.as_deref().is_some_and(|p| p.ends_with(".7z"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetFile {
    /// Member of the archive.
    pub from: String,
    /// Destination, relative to the client patch directory or the server
    /// data directory (`/` separators).
    pub to: String,
}

impl GitSource {
    /// Download URL of one file of the repository at the pinned commit.
    /// Only GitHub repositories are supported.
    pub fn raw_url(&self, path: &str) -> Option<String> {
        let repo = self.repo.strip_prefix("https://github.com/")?;
        let repo = repo.trim_end_matches('/').trim_end_matches(".git");
        Some(format!(
            "https://raw.githubusercontent.com/{repo}/{}/{path}",
            self.commit
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PostInstallStep {
    #[serde(rename_all = "camelCase")]
    CreateAccount {
        #[serde(rename = "as")]
        binding: String,
        username: String,
        #[serde(default)]
        gm_level: u8,
    },
    CreateCharacter {
        #[serde(rename = "as")]
        binding: String,
        account: String,
        name: String,
        #[serde(default)]
        race: Option<String>,
        #[serde(default)]
        class: Option<String>,
    },
    SetConfig {
        values: ConfigChanges,
    },
}

// ------------------------------------------------------------------ client

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientManifest {
    #[serde(rename = "$schema", default, skip_serializing)]
    pub json_schema: Option<String>,
    pub schema_version: u32,
    pub id: String,
    pub name: LocalizedText,
    pub game: Game,
    pub branding: Branding,
    pub version: String,
    pub build: u32,
    pub detection: ClientDetection,
    pub locales: Vec<String>,
    pub realmlist: Realmlist,
    pub patches: DirectoryRef,
    #[serde(default)]
    pub cache: Option<DirectoryRef>,
    /// Where add-ons go (`Interface/AddOns`), when the client has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addons: Option<DirectoryRef>,
    pub server_data: ServerData,
}

/// Presentation of a client in the launcher. Emblems are original artwork
/// bundled with the launcher, never Blizzard logos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Branding {
    pub short_name: LocalizedText,
    pub emblem: String,
    pub accent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Game {
    Wow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientDetection {
    pub executable: String,
    pub build: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Realmlist {
    /// Path template; `{locale}` is replaced.
    pub file: String,
    /// Line template; `{address}` is replaced.
    pub line: String,
    /// A settings file where the client also saves the address: its lines
    /// starting with `prefix` (any case) are rewritten when the file exists.
    #[serde(default)]
    pub saved: Option<SavedSetting>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSetting {
    pub file: String,
    pub prefix: String,
    /// Line template; `{address}` is replaced.
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryRef {
    pub directory: String,
}

/// Pre-extracted server data, downloaded from the pinned upstream release and
/// verified by sha256. Never re-hosted by the project (ADR 0006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerData {
    pub url: String,
    pub version: String,
    pub size: u64,
    /// Approximate size once unpacked (free-space check).
    #[serde(
        rename = "unpackedSize",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unpacked_size: Option<u64>,
    pub sha256: String,
    pub contents: Vec<String>,
}

// --------------------------------------------------------------- component

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentManifest {
    #[serde(rename = "$schema", default, skip_serializing)]
    pub json_schema: Option<String>,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: ComponentKind,
    pub version: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub license: String,
    pub homepage: String,
    /// Keyed by `<os>-<arch>`, e.g. `windows-x86_64`.
    pub platforms: BTreeMap<String, PlatformBuild>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentKind {
    Database,
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformBuild {
    pub upstream: String,
    pub size: u64,
    /// Approximate size once unpacked (free-space check).
    #[serde(
        rename = "unpackedSize",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unpacked_size: Option<u64>,
    pub sha256: String,
    pub executables: BTreeMap<String, String>,
}

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
    pub ports: BTreeMap<String, u16>,
    pub processes: Vec<ProcessSpec>,
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
            | ModuleOption::Choice { id, .. } => id,
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub artifact: String,
    /// Files copied into the client patch directory (tracked, removable).
    #[serde(default)]
    pub client: Vec<String>,
    /// Files overlaid on the instance server data directory.
    #[serde(default)]
    pub server: Vec<String>,
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
    pub version: String,
    pub build: u32,
    pub detection: ClientDetection,
    pub locales: Vec<String>,
    pub realmlist: Realmlist,
    pub patches: DirectoryRef,
    #[serde(default)]
    pub cache: Option<DirectoryRef>,
    pub server_data: ServerData,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryRef {
    pub directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerData {
    pub artifact: String,
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
    pub executables: BTreeMap<String, String>,
}

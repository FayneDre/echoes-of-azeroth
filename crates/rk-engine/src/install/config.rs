//! Config layers of an instance (ADR 0007, architecture §9). Pure.

use std::collections::BTreeMap;
use std::path::Path;

use rk_conf::{Layer, LayerKind};
use rk_core::manifest::{
    Activation, ConfigChanges, ConfigMap, ConfigValue, Database, ModuleOption, OptionValue,
    PostInstallStep, REALM_OPTIONS,
};
use rk_core::template::{self, Variables};
use rk_core::{Catalog, Resolution};

use super::PlanError;

/// Instance-specific values substituted into the manifest's `instance` section.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceValues<'a> {
    pub root: &'a Path,
    pub logs: &'a Path,
    pub sql: &'a Path,
    pub data: &'a Path,
    pub mysql_client: &'a Path,
    /// Game ports by manifest name (`auth`, `world`, `soap`).
    pub ports: &'a BTreeMap<String, u16>,
    /// `127.0.0.1` (solo) or `0.0.0.0` (LAN).
    pub bind: &'a str,
    pub db_port: u16,
    pub db_user: &'a str,
    pub db_password: &'a str,
    /// Option values chosen by the player, by module (`instance.json`).
    pub options: &'a BTreeMap<String, BTreeMap<String, OptionValue>>,
}

/// The config changes one option value makes.
fn option_changes(option: &ModuleOption, value: &OptionValue) -> ConfigChanges {
    let mut changes = ConfigChanges::new();
    match (option, value) {
        (ModuleOption::Boolean { target, .. }, OptionValue::Bool(b)) => {
            changes
                .entry(target.file.clone())
                .or_default()
                .insert(target.key.clone(), ConfigValue::Integer(i64::from(*b)));
        }
        (ModuleOption::Integer { target, .. }, OptionValue::Integer(n)) => {
            changes
                .entry(target.file.clone())
                .or_default()
                .insert(target.key.clone(), ConfigValue::Integer(*n));
        }
        (ModuleOption::Number { target, .. }, OptionValue::Number(n)) => {
            changes
                .entry(target.file.clone())
                .or_default()
                .insert(target.key.clone(), ConfigValue::Number(*n));
        }
        (ModuleOption::Choice { choices, .. }, OptionValue::Choice(id)) => {
            if let Some(choice) = choices.iter().find(|c| &c.id == id) {
                for (file, map) in &choice.config {
                    changes.entry(file.clone()).or_default().extend(map.clone());
                }
            }
        }
        _ => {}
    }
    changes
}

/// One config file to render: `configs/<file>` from `bin/configs/<dist>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigFilePlan {
    pub file: String,
    pub dist: String,
    pub layers: Vec<Layer>,
}

/// Forward slashes: accepted by the servers on Windows, no escaping issues.
fn slash(path: &Path) -> ConfigValue {
    ConfigValue::Text(path.to_string_lossy().replace('\\', "/"))
}

fn layer(kind: LayerKind, source: &str, values: ConfigMap) -> Option<Layer> {
    (!values.is_empty()).then(|| Layer {
        kind,
        source: source.to_owned(),
        values,
    })
}

/// Every database the instance uses: the emulator's plus those of the
/// resolved modules.
pub fn databases(catalog: &Catalog, resolution: &Resolution) -> Result<Vec<Database>, PlanError> {
    let entry = catalog
        .emulator(&resolution.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", resolution.emulator)))?;
    let mut dbs = entry.manifest.databases.clone();
    for id in &resolution.modules {
        if let Some(module) = entry.modules.get(id) {
            dbs.extend(module.database.extra_databases.iter().cloned());
        }
    }
    Ok(dbs)
}

/// Modules whose SQL dbimport applies (`Updates.AllowedModules`, dbimport
/// names): the resolved modules, plus the flavor's other modules that read
/// their own tables while disabled (ADR 0008).
pub fn dbimport_modules(
    catalog: &Catalog,
    resolution: &Resolution,
) -> Result<Vec<String>, PlanError> {
    let entry = catalog
        .emulator(&resolution.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", resolution.emulator)))?;
    let chosen = |id: &str| resolution.modules.iter().any(|m| m == id);
    let mut out: Vec<String> = resolution
        .modules
        .iter()
        .filter_map(|id| entry.modules.get(id))
        .map(|m| m.database.dbimport_id.clone())
        .collect();
    for module in entry.modules.values() {
        if module.compatible_flavors.contains(&resolution.flavor)
            && module.database.apply_when_disabled
            && !chosen(&module.id)
        {
            out.push(module.database.dbimport_id.clone());
        }
    }
    Ok(out)
}

/// The config files of an instance and their layers.
///
/// Every module compatible with the flavor gets its file (all are compiled
/// in, ADR 0003): chosen modules are enabled, the others get their `disable`
/// values, locked.
pub fn plan_configs(
    catalog: &Catalog,
    resolution: &Resolution,
    values: &InstanceValues<'_>,
) -> Result<Vec<ConfigFilePlan>, PlanError> {
    let entry = catalog
        .emulator(&resolution.emulator)
        .ok_or_else(|| PlanError::Unknown(format!("emulator '{}'", resolution.emulator)))?;
    let emu = &entry.manifest;
    let chosen = |id: &str| resolution.modules.iter().any(|m| m == id);
    let modules: Vec<_> = entry
        .modules
        .values()
        .filter(|m| m.compatible_flavors.contains(&resolution.flavor))
        .collect();

    // file -> dist, then file -> layers in application order.
    let mut dists: BTreeMap<String, String> = BTreeMap::new();
    for file in emu.instance.config.keys() {
        dists.insert(file.clone(), format!("{file}.dist"));
    }
    for module in &modules {
        for f in &module.config.files {
            dists.insert(f.file.clone(), f.dist.clone());
        }
    }
    let mut layers: BTreeMap<String, Vec<Layer>> = BTreeMap::new();
    let mut add = |changes: &ConfigChanges, kind: LayerKind, source: &str| {
        for (file, map) in changes {
            if let Some(l) = layer(kind, source, map.clone()) {
                layers.entry(file.clone()).or_default().push(l);
            }
        }
    };

    for module in &modules {
        let on = chosen(&module.id);
        // Defaults first, so activation and options decide over them.
        let defaults: ConfigChanges = module
            .config
            .files
            .iter()
            .map(|f| (f.file.clone(), f.defaults.clone()))
            .collect();
        add(&defaults, LayerKind::ModuleDefault, &module.id);
        match (&module.activation, on) {
            (Activation::Config { enable, .. }, true) => {
                add(enable, LayerKind::ModuleDefault, &module.id)
            }
            (Activation::Config { disable, .. }, false) => {
                add(disable, LayerKind::CoreOverride, &module.id)
            }
            (Activation::Always, _) => {}
        }
        if on {
            // Option defaults, then the player's choices (a value that no
            // longer fits the option falls back to its default).
            let chosen_values = values.options.get(&module.id);
            for option in &module.options {
                add(
                    &option_changes(option, &option.default_value()),
                    LayerKind::ModuleDefault,
                    &module.id,
                );
                if let Some(value) = chosen_values
                    .and_then(|v| v.get(option.id()))
                    .and_then(|v| option.fit(v))
                {
                    add(&option_changes(option, &value), LayerKind::User, &module.id);
                }
            }
            // Post-install results (account ids...): locked, expanded at
            // render time from the instance's bindings (`bindings.rs`).
            for step in &module.post_install {
                if let PostInstallStep::SetConfig { values } = step {
                    add(values, LayerKind::Launcher, &module.id);
                }
            }
        }
    }

    // Realm settings: only the values the player changed (the others stay
    // as upstream ships them). Below the modules' core overrides, which
    // stay locked.
    let realm = values.options.get(REALM_OPTIONS);
    let mut realm_changes = ConfigChanges::new();
    for option in emu.settings.iter().flat_map(|s| &s.options) {
        if let Some(value) = realm
            .and_then(|v| v.get(option.id()))
            .and_then(|v| option.fit(v))
        {
            for (file, map) in option_changes(option, &value) {
                realm_changes.entry(file).or_default().extend(map);
            }
        }
    }
    add(&realm_changes, LayerKind::User, REALM_OPTIONS);

    // Core overrides of the chosen modules, one layer per owner.
    let mut by_owner: BTreeMap<&str, ConfigChanges> = BTreeMap::new();
    for (file, keys) in &resolution.core_overrides {
        for (key, (value, owner)) in keys {
            by_owner
                .entry(owner.as_str())
                .or_default()
                .entry(file.clone())
                .or_default()
                .insert(key.clone(), value.clone());
        }
    }
    for (owner, changes) in &by_owner {
        add(changes, LayerKind::CoreOverride, owner);
    }

    // Launcher: instance values and database connections.
    let mut vars = Variables::from([
        ("dir.instance".to_owned(), slash(values.root)),
        ("dir.logs".to_owned(), slash(values.logs)),
        ("dir.sql".to_owned(), slash(values.sql)),
        ("dir.data".to_owned(), slash(values.data)),
        ("tool.mysql".to_owned(), slash(values.mysql_client)),
        (
            "network.bind".to_owned(),
            ConfigValue::Text(values.bind.to_owned()),
        ),
    ]);
    vars.insert(
        "modules.dbimport".into(),
        ConfigValue::Text(dbimport_modules(catalog, resolution)?.join(",")),
    );
    for name in emu.ports.keys() {
        let port = values
            .ports
            .get(name)
            .ok_or_else(|| PlanError::Unknown(format!("port '{name}' has no value")))?;
        vars.insert(
            format!("port.{name}"),
            ConfigValue::Integer(i64::from(*port)),
        );
    }
    let mut launcher = ConfigChanges::new();
    for (file, keys) in &emu.instance.config {
        let out = launcher.entry(file.clone()).or_default();
        for (key, value) in keys {
            out.insert(key.clone(), template::expand(value, &vars)?);
        }
    }
    for db in databases(catalog, resolution)? {
        let db_vars = Variables::from([
            (
                "db.port".to_owned(),
                ConfigValue::Integer(i64::from(values.db_port)),
            ),
            (
                "db.user".to_owned(),
                ConfigValue::Text(values.db_user.to_owned()),
            ),
            (
                "db.password".to_owned(),
                ConfigValue::Text(values.db_password.to_owned()),
            ),
            ("db.name".to_owned(), ConfigValue::Text(db.name.clone())),
        ]);
        let connection = template::expand_str(&emu.instance.connection, &db_vars)?;
        for c in &db.connections {
            launcher
                .entry(c.file.clone())
                .or_default()
                .insert(c.key.clone(), ConfigValue::Text(connection.clone()));
        }
    }
    add(&launcher, LayerKind::Launcher, "launcher");

    let mut plans = Vec::new();
    for (file, layers) in layers {
        let dist = dists
            .get(&file)
            .cloned()
            .ok_or_else(|| PlanError::Unknown(format!("no .conf.dist declared for '{file}'")))?;
        plans.push(ConfigFilePlan { file, dist, layers });
    }
    // Files with a dist but no layer (nothing to change) are still installed.
    for (file, dist) in dists {
        if !plans.iter().any(|p| p.file == file) {
            plans.push(ConfigFilePlan {
                file,
                dist,
                layers: Vec::new(),
            });
        }
    }
    plans.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(plans)
}

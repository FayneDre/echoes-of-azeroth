//! Config layers of an instance (ADR 0007, architecture §9). Pure.

use std::collections::BTreeMap;
use std::path::Path;

use eoa_conf::{Layer, LayerKind};
use eoa_core::manifest::{
    Activation, ConfigChanges, ConfigMap, ConfigValue, Database, ModuleOption,
};
use eoa_core::template::{self, Variables};
use eoa_core::{Catalog, Resolution};

use super::PlanError;

/// Instance-specific values substituted into the manifest's `instance` section.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            let mut options = ConfigChanges::new();
            for option in &module.options {
                match option {
                    ModuleOption::Boolean {
                        target, default, ..
                    } => {
                        options.entry(target.file.clone()).or_default().insert(
                            target.key.clone(),
                            ConfigValue::Integer(i64::from(*default)),
                        );
                    }
                    ModuleOption::Integer {
                        target, default, ..
                    } => {
                        options
                            .entry(target.file.clone())
                            .or_default()
                            .insert(target.key.clone(), ConfigValue::Integer(*default));
                    }
                    ModuleOption::Choice {
                        default, choices, ..
                    } => {
                        if let Some(choice) = choices.iter().find(|c| &c.id == default) {
                            for (file, map) in &choice.config {
                                options.entry(file.clone()).or_default().extend(map.clone());
                            }
                        }
                    }
                }
            }
            add(&options, LayerKind::ModuleDefault, &module.id);
        }
    }

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
    // Chosen modules, plus those that read their own tables while disabled (ADR 0008).
    let mut dbimport: Vec<&str> = resolution
        .modules
        .iter()
        .filter_map(|id| entry.modules.get(id))
        .map(|m| m.database.dbimport_id.as_str())
        .collect();
    for module in &modules {
        if module.database.apply_when_disabled && !chosen(&module.id) {
            dbimport.push(&module.database.dbimport_id);
        }
    }
    vars.insert(
        "modules.dbimport".into(),
        ConfigValue::Text(dbimport.join(",")),
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

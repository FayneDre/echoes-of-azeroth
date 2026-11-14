use std::collections::BTreeSet;
use std::fmt;

use eoa_core::manifest::{Activation, ModuleOption, PostInstallStep};
use eoa_core::Catalog;

/// A cross-manifest consistency problem. `location` is `emulator/kind/id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub location: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Checks references between manifests. Returns every issue found (empty = valid).
pub fn validate(catalog: &Catalog) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut push = |location: String, message: String| issues.push(Issue { location, message });

    for (emu_id, entry) in &catalog.emulators {
        let emu = &entry.manifest;
        let here = format!("{emu_id}/emulator");

        if !catalog.clients.contains_key(&emu.client) {
            push(here.clone(), format!("unknown client '{}'", emu.client));
        }
        for component in &emu.components {
            if !catalog.components.contains_key(&component.id) {
                push(
                    here.clone(),
                    format!("unknown component '{}'", component.id),
                );
            }
        }
        if !entry.flavors.contains_key(&emu.default_flavor) {
            push(
                here.clone(),
                format!("unknown default flavor '{}'", emu.default_flavor),
            );
        }
        for process in &emu.processes {
            if let eoa_core::manifest::Readiness::Tcp { port } = &process.readiness {
                if !emu.ports.contains_key(port) {
                    push(
                        here.clone(),
                        format!("process '{}' waits on undeclared port '{port}'", process.id),
                    );
                }
            }
        }

        // Instance placeholders (ADR 0007): a closed list, plus declared ports.
        let mut check_template = |text: &str, allowed: &dyn Fn(&str) -> bool, what: &str| {
            match eoa_core::template::placeholders(text) {
                Err(e) => push(here.clone(), format!("{what}: {e}")),
                Ok(names) => {
                    for name in names.into_iter().filter(|n| !allowed(n)) {
                        push(
                            here.clone(),
                            format!("{what}: unknown placeholder '${{{name}}}'"),
                        );
                    }
                }
            }
        };
        let instance_var = |name: &str| {
            eoa_core::template::INSTANCE_VARIABLES.contains(&name)
                || name
                    .strip_prefix("port.")
                    .is_some_and(|port| emu.ports.contains_key(port))
        };
        let connection_var = |name: &str| eoa_core::template::CONNECTION_VARIABLES.contains(&name);
        check_template(
            &emu.instance.connection,
            &connection_var,
            "instance.connection",
        );
        for (file, keys) in &emu.instance.config {
            for (key, value) in keys {
                if let eoa_core::manifest::ConfigValue::Text(text) = value {
                    check_template(
                        text,
                        &instance_var,
                        &format!("instance.config {file} {key}"),
                    );
                }
            }
        }

        let mut database_ids: BTreeSet<&str> =
            emu.databases.iter().map(|d| d.id.as_str()).collect();
        for module in entry.modules.values() {
            database_ids.extend(
                module
                    .database
                    .extra_databases
                    .iter()
                    .map(|d| d.id.as_str()),
            );
        }

        for flavor in entry.flavors.values() {
            let here = format!("{emu_id}/flavor/{}", flavor.id);
            for id in &flavor.required_modules {
                match entry.modules.get(id) {
                    None => push(here.clone(), format!("unknown required module '{id}'")),
                    Some(m) if !m.compatible_flavors.contains(&flavor.id) => push(
                        here.clone(),
                        format!("required module '{id}' does not list this flavor as compatible"),
                    ),
                    Some(m) if m.removable => push(
                        here.clone(),
                        format!("required module '{id}' must be \"removable\": false"),
                    ),
                    Some(_) => {}
                }
            }
            // Whatever a required module pulls in is mandatory too (the UI
            // shows it as included), so it must not be removable either.
            for id in catalog.mandatory_modules(emu_id, &flavor.id) {
                if flavor.required_modules.contains(&id) {
                    continue;
                }
                if let Some(m) = entry.modules.get(&id) {
                    if m.removable {
                        push(
                            here.clone(),
                            format!(
                                "module '{id}' is required through a required module and must be \"removable\": false"
                            ),
                        );
                    }
                }
            }
        }

        for module in entry.modules.values() {
            let here = format!("{emu_id}/module/{}", module.id);
            for flavor in &module.compatible_flavors {
                if !entry.flavors.contains_key(flavor) {
                    push(
                        here.clone(),
                        format!("unknown compatible flavor '{flavor}'"),
                    );
                }
            }
            for id in module.requires.iter().chain(&module.conflicts) {
                if !entry.modules.contains_key(id) {
                    push(here.clone(), format!("references unknown module '{id}'"));
                }
                if id == &module.id {
                    push(here.clone(), "references itself".into());
                }
            }
            if matches!(module.activation, Activation::Always) && module.removable {
                push(
                    here.clone(),
                    "activation 'always' requires \"removable\": false".into(),
                );
            }
            for db in &module.database.touches {
                if !database_ids.contains(db.as_str()) {
                    push(here.clone(), format!("touches unknown database '{db}'"));
                }
            }

            let asset_ids: BTreeSet<&str> = module.assets.iter().map(|a| a.id.as_str()).collect();
            let mut option_ids = BTreeSet::new();
            for option in &module.options {
                if !option_ids.insert(option.id()) {
                    push(here.clone(), format!("duplicate option '{}'", option.id()));
                }
                if let ModuleOption::Choice {
                    id,
                    default,
                    choices,
                    ..
                } = option
                {
                    if !choices.iter().any(|c| &c.id == default) {
                        push(
                            here.clone(),
                            format!("option '{id}': default '{default}' is not a choice"),
                        );
                    }
                    for choice in choices {
                        for asset in &choice.assets {
                            if !asset_ids.contains(asset.as_str()) {
                                push(
                                    here.clone(),
                                    format!("option '{id}': unknown asset '{asset}'"),
                                );
                            }
                        }
                    }
                }
                if let ModuleOption::Integer {
                    id,
                    default,
                    min,
                    max,
                    ..
                } = option
                {
                    if min.is_some_and(|m| *default < m) || max.is_some_and(|m| *default > m) {
                        push(
                            here.clone(),
                            format!("option '{id}': default {default} is out of range"),
                        );
                    }
                }
            }

            let mut bindings = BTreeSet::new();
            for step in &module.post_install {
                match step {
                    PostInstallStep::CreateAccount { binding, .. } => {
                        bindings.insert(binding.as_str());
                    }
                    PostInstallStep::CreateCharacter {
                        binding, account, ..
                    } => {
                        if !bindings.contains(account.as_str()) {
                            push(
                                here.clone(),
                                format!("create-character uses unbound account '{account}'"),
                            );
                        }
                        bindings.insert(binding.as_str());
                    }
                    PostInstallStep::SetConfig { values } => {
                        for value in values.values().flat_map(|keys| keys.values()) {
                            if let eoa_core::manifest::ConfigValue::Text(text) = value {
                                for reference in placeholders(text) {
                                    let name = reference.split('.').next().unwrap_or_default();
                                    if !bindings.contains(name) {
                                        push(
                                            here.clone(),
                                            format!("set-config uses unbound '${{{reference}}}'"),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    issues
}

/// Extracts `name.field` from every `${name.field}` in `text`.
fn placeholders(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                out.push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::placeholders;

    #[test]
    fn extracts_placeholders() {
        assert_eq!(placeholders("${a.id}-${b.guid}"), vec!["a.id", "b.guid"]);
        assert!(placeholders("plain").is_empty());
    }
}

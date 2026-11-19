use std::collections::BTreeSet;
use std::fmt;

use rk_core::manifest::{Activation, ModuleOption, PostInstallStep};
use rk_core::Catalog;

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

/// `relativePath` of `module.schema.json`: `/`-separated segments of ASCII
/// letters, digits, `.`, `_` or `-`, never `.` or `..`.
fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
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
        // Server data (ADR 0006, 0024): downloaded for the client, or
        // extracted from it with known placeholders, in a locale it has.
        match &emu.data_extraction {
            None => {
                if catalog
                    .clients
                    .get(&emu.client)
                    .is_some_and(|c| c.server_data.is_none())
                {
                    push(
                        here.clone(),
                        format!(
                            "no server data: client '{}' has no serverData and no dataExtraction is declared",
                            emu.client
                        ),
                    );
                }
            }
            Some(extraction) => {
                if let Some(client) = catalog.clients.get(&emu.client) {
                    if !client.locales.contains(&extraction.locale) {
                        push(
                            here.clone(),
                            format!(
                                "data extraction needs locale '{}', which client '{}' does not list",
                                extraction.locale, emu.client
                            ),
                        );
                    }
                }
                let mut step_ids = BTreeSet::new();
                for step in &extraction.steps {
                    if !step_ids.insert(&step.id) {
                        push(
                            here.clone(),
                            format!("extraction step '{}' declared twice", step.id),
                        );
                    }
                    for arg in &step.args {
                        match rk_core::template::placeholders(arg) {
                            Ok(names) => {
                                for name in names {
                                    if !rk_core::manifest::EXTRACTION_PLACEHOLDERS.contains(&name) {
                                        push(
                                            here.clone(),
                                            format!(
                                                "extraction step '{}' uses unknown '${{{name}}}'",
                                                step.id
                                            ),
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                push(here.clone(), format!("extraction step '{}': {e}", step.id))
                            }
                        }
                    }
                }
            }
        }

        // GM panel: every placeholder is a parameter, ids are unique, and
        // start milestones only move forward.
        let mut action_ids = std::collections::BTreeSet::new();
        for action in &emu.console_actions {
            if !action_ids.insert(&action.id) {
                push(
                    here.clone(),
                    format!("console action '{}' declared twice", action.id),
                );
            }
            for name in rk_core::console::placeholders(&action.command) {
                if !action.params.iter().any(|p| p.id == name) {
                    push(
                        here.clone(),
                        format!(
                            "console action '{}' uses undeclared '{{{name}}}'",
                            action.id
                        ),
                    );
                }
            }
            for param in &action.params {
                if let rk_core::manifest::ConsoleParamKind::Number { min, max, default } =
                    &param.kind
                {
                    if min > max || default.is_some_and(|d| d < *min || d > *max) {
                        push(
                            here.clone(),
                            format!(
                                "console action '{}': bad range for '{}'",
                                action.id, param.id
                            ),
                        );
                    }
                }
            }
        }
        for process in &emu.processes {
            if process
                .startup
                .windows(2)
                .any(|w| w[1].progress < w[0].progress)
            {
                push(
                    here.clone(),
                    format!("process '{}': startup milestones go backwards", process.id),
                );
            }
            if let rk_core::manifest::Readiness::Tcp { port } = &process.readiness {
                if !emu.ports.contains_key(port) {
                    push(
                        here.clone(),
                        format!("process '{}' waits on undeclared port '{port}'", process.id),
                    );
                }
            }
        }

        // Realm settings (ADR 0017): core config files only, never a key the
        // launcher imposes, no assets, unique ids.
        if entry.modules.contains_key(rk_core::manifest::REALM_OPTIONS) {
            push(
                here.clone(),
                format!(
                    "module id '{}' is reserved for the realm settings",
                    rk_core::manifest::REALM_OPTIONS
                ),
            );
        }
        let mut setting_ids = BTreeSet::new();
        for option in emu.settings.iter().flat_map(|s| &s.options) {
            let id = option.id();
            if !setting_ids.insert(id) {
                push(here.clone(), format!("duplicate realm setting '{id}'"));
            }
            let mut targets: Vec<(&str, &str)> = Vec::new();
            match option {
                rk_core::manifest::ModuleOption::Boolean { target, .. }
                | rk_core::manifest::ModuleOption::Integer { target, .. }
                | rk_core::manifest::ModuleOption::Number { target, .. } => {
                    targets.push((&target.file, &target.key))
                }
                rk_core::manifest::ModuleOption::Choice { choices, .. } => {
                    for choice in choices {
                        if !choice.assets.is_empty() {
                            push(
                                here.clone(),
                                format!("realm setting '{id}': choices cannot install assets"),
                            );
                        }
                        for (file, keys) in &choice.config {
                            targets.extend(keys.keys().map(|k| (file.as_str(), k.as_str())));
                        }
                    }
                }
            }
            for (file, key) in targets {
                match emu.instance.config.get(file) {
                    None => push(
                        here.clone(),
                        format!("realm setting '{id}': '{file}' is not a core config file"),
                    ),
                    Some(locked) if locked.contains_key(key) => push(
                        here.clone(),
                        format!("realm setting '{id}': '{key}' is imposed by the launcher"),
                    ),
                    Some(_) => {}
                }
            }
        }

        // Instance placeholders (ADR 0007): a closed list, plus declared ports.
        let mut check_template = |text: &str, allowed: &dyn Fn(&str) -> bool, what: &str| {
            match rk_core::template::placeholders(text) {
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
            rk_core::template::INSTANCE_VARIABLES.contains(&name)
                || name
                    .strip_prefix("port.")
                    .is_some_and(|port| emu.ports.contains_key(port))
        };
        let connection_var = |name: &str| rk_core::template::CONNECTION_VARIABLES.contains(&name);
        check_template(
            &emu.instance.connection,
            &connection_var,
            "instance.connection",
        );
        for (file, keys) in &emu.instance.config {
            for (key, value) in keys {
                if let rk_core::manifest::ConfigValue::Text(text) = value {
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
            // Asset paths end up joined to the client and data folders: the
            // schema's rule again, so no manifest can write outside them.
            for asset in &module.assets {
                if asset.source.url(&module.source).is_none() {
                    push(
                        here.clone(),
                        format!(
                            "asset '{}': source needs either 'path' or a GitHub 'repository' and its 'commit'",
                            asset.id
                        ),
                    );
                }
                if let Some(commit) = &asset.source.commit {
                    if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
                        push(
                            here.clone(),
                            format!("asset '{}': commit must be 40 hex digits", asset.id),
                        );
                    }
                }
                let files = asset
                    .client
                    .iter()
                    .chain(&asset.server)
                    .chain(&asset.addons);
                let paths = asset
                    .source
                    .path
                    .iter()
                    .map(String::as_str)
                    .chain(files.flat_map(|f| [f.from.as_str(), f.to.as_str()]));
                for path in paths {
                    if !is_safe_relative(path) {
                        push(
                            here.clone(),
                            format!("asset '{}': unsafe path '{path}'", asset.id),
                        );
                    }
                }
            }
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
                            if let rk_core::manifest::ConfigValue::Text(text) = value {
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

//! Module resolution: user selection -> deterministic installation set.
//!
//! Rules:
//! - the flavor's `requiredModules` are always included (e.g. Playerbots);
//! - `requires` are added transitively;
//! - every module must list the flavor in `compatibleFlavors`;
//! - `conflicts` are rejected (declared on either side);
//! - two modules may not impose different values on the same core key;
//! - the output order puts dependencies before dependents, ties broken by id.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::catalog::Catalog;
use crate::manifest::ConfigValue;

#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    pub emulator: String,
    pub flavor: String,
    /// Installation order (dependencies first).
    pub modules: Vec<String>,
    /// Merged core overrides: file -> key -> (value, imposing module).
    pub core_overrides: BTreeMap<String, BTreeMap<String, (ConfigValue, String)>>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ResolveError {
    #[error("unknown emulator '{0}'")]
    UnknownEmulator(String),
    #[error("unknown flavor '{flavor}' for emulator '{emulator}'")]
    UnknownFlavor { emulator: String, flavor: String },
    #[error("unknown module '{0}'")]
    UnknownModule(String),
    #[error("module '{module}' is not compatible with flavor '{flavor}'")]
    IncompatibleFlavor { module: String, flavor: String },
    #[error("modules '{0}' and '{1}' conflict")]
    Conflict(String, String),
    #[error("modules '{first}' and '{second}' set different values for '{key}' in '{file}'")]
    OverrideConflict {
        file: String,
        key: String,
        first: String,
        second: String,
    },
    #[error("dependency cycle involving: {0:?}")]
    Cycle(Vec<String>),
}

/// Resolves `selected` modules for `emulator`/`flavor` (`None` = default flavor).
pub fn resolve(
    catalog: &Catalog,
    emulator: &str,
    flavor: Option<&str>,
    selected: &[&str],
) -> Result<Resolution, ResolveError> {
    let entry = catalog
        .emulator(emulator)
        .ok_or_else(|| ResolveError::UnknownEmulator(emulator.to_owned()))?;
    let flavor_id = flavor.unwrap_or(&entry.manifest.default_flavor).to_owned();
    let flavor = entry
        .flavors
        .get(&flavor_id)
        .ok_or_else(|| ResolveError::UnknownFlavor {
            emulator: emulator.to_owned(),
            flavor: flavor_id.clone(),
        })?;

    // 1. Closure over required + selected + transitive `requires`.
    let mut set = BTreeSet::new();
    let mut stack: Vec<String> = flavor.required_modules.clone();
    stack.extend(selected.iter().map(|s| (*s).to_owned()));
    while let Some(id) = stack.pop() {
        if !set.insert(id.clone()) {
            continue;
        }
        let module = entry
            .modules
            .get(&id)
            .ok_or_else(|| ResolveError::UnknownModule(id.clone()))?;
        stack.extend(module.requires.iter().cloned());
    }

    // 2. Compatibility and conflicts.
    for id in &set {
        let module = &entry.modules[id];
        if !module.compatible_flavors.contains(&flavor_id) {
            return Err(ResolveError::IncompatibleFlavor {
                module: id.clone(),
                flavor: flavor_id,
            });
        }
        if let Some(other) = module.conflicts.iter().find(|c| set.contains(*c)) {
            let (a, b) = ordered(id, other);
            return Err(ResolveError::Conflict(a, b));
        }
    }

    // 3. Deterministic topological order (Kahn, BTreeSet for stable ties).
    let mut pending: BTreeMap<&str, BTreeSet<&str>> = set
        .iter()
        .map(|id| {
            let deps = entry.modules[id]
                .requires
                .iter()
                .map(String::as_str)
                .collect();
            (id.as_str(), deps)
        })
        .collect();
    let mut order = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let ready: Vec<&str> = pending
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            return Err(ResolveError::Cycle(
                pending.keys().map(|k| (*k).to_owned()).collect(),
            ));
        }
        for id in ready {
            pending.remove(id);
            for deps in pending.values_mut() {
                deps.remove(id);
            }
            order.push(id.to_owned());
        }
    }

    // 4. Merge core overrides, rejecting contradictions.
    let mut core_overrides: BTreeMap<String, BTreeMap<String, (ConfigValue, String)>> =
        BTreeMap::new();
    for id in &order {
        for (file, keys) in &entry.modules[id].config.core_overrides {
            let merged = core_overrides.entry(file.clone()).or_default();
            for (key, value) in keys {
                match merged.get(key) {
                    Some((existing, owner)) if existing != value => {
                        return Err(ResolveError::OverrideConflict {
                            file: file.clone(),
                            key: key.clone(),
                            first: owner.clone(),
                            second: id.clone(),
                        });
                    }
                    Some(_) => {}
                    None => {
                        merged.insert(key.clone(), (value.clone(), id.clone()));
                    }
                }
            }
        }
    }

    Ok(Resolution {
        emulator: emulator.to_owned(),
        flavor: flavor_id,
        modules: order,
        core_overrides,
    })
}

fn ordered(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_owned(), b.to_owned())
    } else {
        (b.to_owned(), a.to_owned())
    }
}

//! Results of module post-install steps (`instances/<id>/bindings.json`).
//!
//! `create-account` / `create-character` steps store what they created under
//! their binding name (`ahbot_account.id`, `ahbot_character.guid`), per
//! module. `set-config` values reference them and are expanded when configs
//! are rendered; a value whose binding does not exist yet is left out, so the
//! key keeps its upstream default until the post-install step has run.

use std::collections::BTreeMap;
use std::path::Path;

use rk_conf::Layer;
use rk_core::manifest::ConfigValue;
use rk_core::template::{self, TemplateError, Variables};
use serde::{Deserialize, Serialize};

use super::{read_json, write_json, PlanError};

/// Module id -> `binding.field` -> value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bindings(pub BTreeMap<String, BTreeMap<String, u64>>);

impl Bindings {
    pub fn load(path: &Path) -> Result<Self, PlanError> {
        Ok(read_json(path)?.unwrap_or_default())
    }

    /// Written atomically: a crash leaves the previous or the new state.
    pub fn save(&self, path: &Path) -> Result<(), PlanError> {
        write_json(path, self)
    }

    pub fn get(&self, module: &str, key: &str) -> Option<u64> {
        self.0.get(module)?.get(key).copied()
    }

    pub fn set(&mut self, module: &str, key: &str, value: u64) {
        self.0
            .entry(module.to_owned())
            .or_default()
            .insert(key.to_owned(), value);
    }

    fn variables(&self, module: &str) -> Variables {
        self.0
            .get(module)
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), ConfigValue::Integer(*v as i64)))
            .collect()
    }

    /// Expands the placeholders left in layers (`set-config` values, whose
    /// layer source is the module id). Values with a missing binding are
    /// dropped.
    pub fn resolve(&self, layers: &[Layer]) -> Result<Vec<Layer>, TemplateError> {
        let mut out = Vec::with_capacity(layers.len());
        for layer in layers {
            let vars = self.variables(&layer.source);
            let mut resolved = layer.clone();
            resolved.values.clear();
            for (key, value) in &layer.values {
                match value {
                    ConfigValue::Text(text) if text.contains("${") => {
                        match template::expand(value, &vars) {
                            Ok(value) => {
                                resolved.values.insert(key.clone(), value);
                            }
                            Err(TemplateError::Unknown(_)) => {}
                            Err(error) => return Err(error),
                        }
                    }
                    _ => {
                        resolved.values.insert(key.clone(), value.clone());
                    }
                }
            }
            out.push(resolved);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_conf::LayerKind;

    #[test]
    fn placeholders_resolve_per_module_or_drop_out() {
        let layer = Layer {
            kind: LayerKind::Launcher,
            source: "mod-ah-bot".into(),
            values: [
                ("A".into(), ConfigValue::Text("${acc.id}".into())),
                ("G".into(), ConfigValue::Text("${chr.guid}".into())),
                ("K".into(), ConfigValue::Integer(1)),
            ]
            .into_iter()
            .collect(),
        };
        let mut bindings = Bindings::default();
        bindings.set("mod-ah-bot", "acc.id", 7);
        bindings.set("other", "chr.guid", 9); // another module's binding
        let out = bindings.resolve(&[layer]).unwrap();
        assert_eq!(out[0].values.get("A"), Some(&ConfigValue::Integer(7)));
        assert_eq!(out[0].values.get("G"), None);
        assert_eq!(out[0].values.get("K"), Some(&ConfigValue::Integer(1)));
    }
}

use std::collections::BTreeMap;

use eoa_core::manifest::{ConfigMap, ConfigValue};

use crate::file::{canonical, ConfError, ConfFile};

/// Where a layer comes from. Determines whether the UI lets the player change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerKind {
    /// Defaults a module manifest sets for its own config file.
    ModuleDefault,
    /// Values chosen by the player (UI or imported manual edits).
    User,
    /// Values a module requires in a core file (e.g. Individual Progression's
    /// `EnablePlayerSettings = 1`). Locked while the module is active.
    CoreOverride,
    /// Values the launcher must control (paths, ports, database credentials,
    /// `Updates.EnableDatabases = 0`). Always locked.
    Launcher,
}

impl LayerKind {
    /// Locked keys are shown read-only, with their [`Provenance`] as the reason.
    pub fn is_locked(self) -> bool {
        matches!(self, LayerKind::CoreOverride | LayerKind::Launcher)
    }
}

/// One set of values applied on top of the `.conf.dist`.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub kind: LayerKind,
    /// Who set it: a module id, `"user"`, `"launcher"`.
    pub source: String,
    pub values: ConfigMap,
}

/// Who decided the final value of a key.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    pub kind: LayerKind,
    pub source: String,
    pub value: ConfigValue,
    /// Layers whose value for this key was overridden (lowest first).
    pub shadowed: Vec<(String, ConfigValue)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub text: String,
    /// Every key set by a layer (keys only from the `.conf.dist` are absent).
    pub provenance: BTreeMap<String, Provenance>,
}

/// Renders the effective file: the upstream `dist` text with `layers` applied
/// in order of [`LayerKind`] (module defaults < user < core overrides < launcher),
/// then in the given order within the same kind.
///
/// The output is a pure function of its inputs, so re-rendering after an update
/// of the `.conf.dist` picks up new upstream keys and documentation.
pub fn render(dist: &str, layers: &[Layer]) -> Result<Rendered, ConfError> {
    let mut ordered: Vec<&Layer> = layers.iter().collect();
    ordered.sort_by_key(|layer| layer.kind); // stable: keeps order within a kind

    let mut provenance: BTreeMap<String, Provenance> = BTreeMap::new();
    for layer in ordered {
        for (key, value) in &layer.values {
            let next = Provenance {
                kind: layer.kind,
                source: layer.source.clone(),
                value: value.clone(),
                shadowed: Vec::new(),
            };
            match provenance.get_mut(key) {
                Some(current) => {
                    let previous = std::mem::replace(current, next);
                    current.shadowed = previous.shadowed;
                    current.shadowed.push((previous.source, previous.value));
                }
                None => {
                    provenance.insert(key.clone(), next);
                }
            }
        }
    }

    let mut file = ConfFile::parse(dist);
    for (key, entry) in &provenance {
        file.set(key, &entry.value)?;
    }
    Ok(Rendered {
        text: file.render(),
        provenance,
    })
}

/// Keys whose value in `current` (the file on disk) differs from `previous`
/// (what the launcher last wrote), including keys the player added.
///
/// The launcher stores these as [`LayerKind::User`] values before re-rendering,
/// so hand edits survive updates. Keys deleted by hand are not reported: the
/// upstream default applies again, which is what deleting a line means upstream.
pub fn manual_changes(previous: &str, current: &str) -> ConfigMap {
    let previous = ConfFile::parse(previous);
    let current = ConfFile::parse(current);
    let mut changes = ConfigMap::new();
    for key in current.keys() {
        if changes.contains_key(key) {
            continue;
        }
        let now = current.raw(key).expect("key comes from this file");
        let unchanged = previous
            .raw(key)
            .is_some_and(|before| canonical(before) == canonical(now));
        if !unchanged {
            changes.insert(key.to_owned(), parse_value(now));
        }
    }
    changes
}

/// Interprets a raw value: quoted -> text, otherwise integer, number or text.
fn parse_value(raw: &str) -> ConfigValue {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return ConfigValue::Text(inner.to_owned());
    }
    if let Ok(v) = raw.parse::<i64>() {
        return ConfigValue::Integer(v);
    }
    if let Ok(v) = raw.parse::<f64>() {
        if v.is_finite() {
            return ConfigValue::Number(v);
        }
    }
    ConfigValue::Text(raw.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIST: &str = "\
# Upstream documentation
EnablePlayerSettings = 0
DBC.EnforceItemAttributes = 1
Rate.XP.Kill = 1
DataDir = \".\"
";

    fn layer(kind: LayerKind, source: &str, values: &[(&str, ConfigValue)]) -> Layer {
        Layer {
            kind,
            source: source.into(),
            values: values
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    #[test]
    fn layers_apply_by_kind_then_order() {
        let layers = [
            // Given out of order on purpose: kind decides precedence.
            layer(
                LayerKind::CoreOverride,
                "mod-individual-progression",
                &[("EnablePlayerSettings", ConfigValue::Integer(1))],
            ),
            layer(
                LayerKind::User,
                "user",
                &[
                    ("EnablePlayerSettings", ConfigValue::Integer(0)),
                    ("Rate.XP.Kill", ConfigValue::Integer(3)),
                ],
            ),
            layer(
                LayerKind::Launcher,
                "launcher",
                &[("DataDir", ConfigValue::Text("D:/data".into()))],
            ),
        ];
        let out = render(DIST, &layers).unwrap();
        assert!(out.text.contains("EnablePlayerSettings = 1\n"));
        assert!(out.text.contains("Rate.XP.Kill = 3\n"));
        assert!(out.text.contains("DataDir = \"D:/data\"\n"));
        assert!(out.text.contains("DBC.EnforceItemAttributes = 1\n"));
        assert!(out.text.starts_with("# Upstream documentation\n"));

        let eps = &out.provenance["EnablePlayerSettings"];
        assert_eq!(eps.source, "mod-individual-progression");
        assert!(eps.kind.is_locked());
        assert_eq!(eps.shadowed, vec![("user".into(), ConfigValue::Integer(0))]);
        assert!(!out.provenance["Rate.XP.Kill"].kind.is_locked());
        assert!(!out.provenance.contains_key("DBC.EnforceItemAttributes"));
    }

    #[test]
    fn rendering_is_deterministic_and_idempotent() {
        let layers = [layer(
            LayerKind::User,
            "user",
            &[("Rate.XP.Kill", ConfigValue::Integer(2))],
        )];
        let a = render(DIST, &layers).unwrap();
        let b = render(DIST, &layers).unwrap();
        assert_eq!(a, b);
        assert!(manual_changes(&a.text, &a.text).is_empty());
    }

    #[test]
    fn manual_edits_are_detected_and_survive_rerender() {
        let first = render(DIST, &[]).unwrap().text;
        let edited = first
            .replace("Rate.XP.Kill = 1", "Rate.XP.Kill = 5")
            .replace("DataDir = \".\"", "DataDir = \"E:/wow\"")
            + "Custom.Key = 0.5\n";
        let changes = manual_changes(&first, &edited);
        assert_eq!(
            changes,
            ConfigMap::from([
                ("Custom.Key".into(), ConfigValue::Number(0.5)),
                ("DataDir".into(), ConfigValue::Text("E:/wow".into())),
                ("Rate.XP.Kill".into(), ConfigValue::Integer(5)),
            ])
        );

        let again = render(
            DIST,
            &[Layer {
                kind: LayerKind::User,
                source: "user".into(),
                values: changes,
            }],
        )
        .unwrap();
        assert!(again.text.contains("Rate.XP.Kill = 5\n"));
        assert!(again.text.contains("Custom.Key = 0.5\n"));
    }

    #[test]
    fn quoting_and_spacing_differences_are_not_changes() {
        let previous = "A = 1\nB = \"x\"\n";
        let current = "A=1\n  B =   \"x\"  \n";
        assert!(manual_changes(previous, current).is_empty());
    }
}

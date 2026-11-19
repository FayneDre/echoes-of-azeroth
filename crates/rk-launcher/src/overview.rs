//! What the app shows of the catalog: each emulator with its client, server
//! processes, modules and GM panel commands. Shared by the app (and any
//! future `--json` output of the CLI), so the Tauri layer only forwards it.

use rk_core::manifest::{ConsoleAction, ModuleCategory, StopMethod};
use rk_core::LocalizedText;
use serde::Serialize;

use crate::Launcher;

#[derive(Debug, Clone, Serialize)]
pub struct ModuleSummary {
    pub id: String,
    pub name: LocalizedText,
    pub description: LocalizedText,
    pub category: ModuleCategory,
    pub homepage: String,
    pub mandatory: bool,
}

/// How the side bar presents the client an emulator serves.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSummary {
    pub id: String,
    pub name: LocalizedText,
    pub short_name: LocalizedText,
    pub version: String,
    pub build: u32,
    pub emblem: String,
    pub accent: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorOverview {
    pub id: String,
    pub name: String,
    pub description: LocalizedText,
    pub client: Option<ClientSummary>,
    /// Server processes in start order (the database is implicit and first).
    pub processes: Vec<String>,
    /// The process whose stdin is the server console (stopped by closing
    /// stdin, ADR 0011), if any.
    pub console: Option<String>,
    pub modules: Vec<ModuleSummary>,
    /// Ready-made console commands of the GM panel (ADR 0020).
    pub console_actions: Vec<ConsoleAction>,
}

impl Launcher {
    /// Every emulator of the catalog, as the app presents it. Modules are
    /// those of the default flavor, mandatory ones first, then by id.
    pub fn catalog_overview(&self) -> Vec<EmulatorOverview> {
        let catalog = self.catalog();
        catalog
            .emulators
            .values()
            .map(|entry| {
                let manifest = &entry.manifest;
                let mandatory = catalog.mandatory_modules(&manifest.id, &manifest.default_flavor);
                let mut modules: Vec<ModuleSummary> = entry
                    .modules
                    .values()
                    .filter(|m| m.compatible_flavors.contains(&manifest.default_flavor))
                    .map(|m| ModuleSummary {
                        id: m.id.clone(),
                        name: m.name.clone(),
                        description: m.description.clone(),
                        category: m.category,
                        homepage: m.homepage.clone(),
                        mandatory: mandatory.contains(&m.id),
                    })
                    .collect();
                modules.sort_by(|a, b| b.mandatory.cmp(&a.mandatory).then_with(|| a.id.cmp(&b.id)));
                let client = catalog
                    .clients
                    .get(&manifest.client)
                    .map(|c| ClientSummary {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        short_name: c.branding.short_name.clone(),
                        version: c.version.clone(),
                        build: c.build,
                        emblem: c.branding.emblem.clone(),
                        accent: c.branding.accent.clone(),
                    });
                EmulatorOverview {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                    client,
                    processes: manifest.processes.iter().map(|p| p.id.clone()).collect(),
                    console: manifest
                        .processes
                        .iter()
                        .find(|p| matches!(p.stop, StopMethod::CloseStdin { .. }))
                        .map(|p| p.id.clone()),
                    console_actions: manifest.console_actions.clone(),
                    modules,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_real_catalog_lists_mandatory_modules_first() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let dir = tempfile::tempdir().unwrap();
        let launcher = crate::Launcher::open(&root, dir.path()).unwrap();
        let overview = launcher.catalog_overview();
        let ac = overview.iter().find(|e| e.id == "azerothcore").unwrap();
        assert_eq!(ac.console.as_deref(), Some("worldserver"));
        let first_optional = ac.modules.iter().position(|m| !m.mandatory).unwrap();
        assert!(ac.modules[..first_optional]
            .iter()
            .any(|m| m.id == "mod-playerbots"));
        assert!(ac.modules[first_optional..].iter().all(|m| !m.mandatory));
        assert!(ac.client.as_ref().is_some_and(|c| c.build == 12340));
    }
}

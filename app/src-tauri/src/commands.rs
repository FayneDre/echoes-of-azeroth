//! Commands exposed to the UI. Keep in sync with `app/src/lib/api.ts`.

use std::path::PathBuf;

use eoa_core::manifest::ModuleCategory;
use eoa_core::LocalizedText;
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct ModuleSummary {
    id: String,
    name: LocalizedText,
    description: LocalizedText,
    category: ModuleCategory,
    homepage: String,
    mandatory: bool,
}

#[derive(Serialize)]
pub struct EmulatorOverview {
    id: String,
    name: String,
    description: LocalizedText,
    modules: Vec<ModuleSummary>,
}

/// Catalog location: bundled resource in release builds, repository copy in dev.
/// Phase 2 replaces this with the signed remote index cached on disk.
fn catalog_dir(app: &AppHandle) -> PathBuf {
    if let Ok(resources) = app.path().resource_dir() {
        let bundled = resources.join("catalog");
        if bundled.join("emulators").is_dir() {
            return bundled;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog")
}

#[tauri::command]
pub fn catalog_overview(app: AppHandle) -> Result<Vec<EmulatorOverview>, String> {
    let catalog = eoa_catalog::load_dir(&catalog_dir(&app)).map_err(|e| e.to_string())?;
    Ok(catalog
        .emulators
        .values()
        .map(|entry| {
            let mandatory =
                catalog.mandatory_modules(&entry.manifest.id, &entry.manifest.default_flavor);
            let mut modules: Vec<ModuleSummary> = entry
                .modules
                .values()
                .filter(|m| {
                    m.compatible_flavors
                        .contains(&entry.manifest.default_flavor)
                })
                .map(|m| ModuleSummary {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    description: m.description.clone(),
                    category: m.category,
                    homepage: m.homepage.clone(),
                    mandatory: mandatory.contains(&m.id),
                })
                .collect();
            // Mandatory modules first, then alphabetical by id.
            modules.sort_by(|a, b| b.mandatory.cmp(&a.mandatory).then_with(|| a.id.cmp(&b.id)));
            EmulatorOverview {
                id: entry.manifest.id.clone(),
                name: entry.manifest.name.clone(),
                description: entry.manifest.description.clone(),
                modules,
            }
        })
        .collect())
}

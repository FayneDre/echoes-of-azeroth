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

/// How the side bar presents the client an emulator serves.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSummary {
    id: String,
    name: LocalizedText,
    short_name: LocalizedText,
    version: String,
    build: u32,
    emblem: String,
    accent: String,
}

#[derive(Serialize)]
pub struct EmulatorOverview {
    id: String,
    name: String,
    description: LocalizedText,
    client: Option<ClientSummary>,
    /// Server processes in start order (the database is implicit and first).
    processes: Vec<String>,
    modules: Vec<ModuleSummary>,
}

/// Failure reported to the UI: `code` selects a translated message
/// (`error.<code>` in the locales); `detail` is technical context, shown
/// folded for bug reports, never as the main message.
#[derive(Serialize)]
pub struct CommandError {
    code: &'static str,
    detail: String,
}

impl CommandError {
    fn new(code: &'static str, detail: impl ToString) -> Self {
        Self {
            code,
            detail: detail.to_string(),
        }
    }
}

/// Catalog location: the bundled resource. Debug builds fall back to the
/// repository copy (the path is compiled into debug binaries only).
/// Phase 2 replaces this with the signed remote index cached on disk.
fn catalog_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(resources) = app.path().resource_dir() {
        let bundled = resources.join("catalog");
        if bundled.join("emulators").is_dir() {
            return Some(bundled);
        }
    }
    #[cfg(debug_assertions)]
    {
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog"))
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

#[tauri::command]
pub fn catalog_overview(app: AppHandle) -> Result<Vec<EmulatorOverview>, CommandError> {
    let dir = catalog_dir(&app)
        .ok_or_else(|| CommandError::new("catalog.missing", "bundled catalog not found"))?;
    let catalog =
        eoa_catalog::load_dir(&dir).map_err(|e| CommandError::new("catalog.unreadable", e))?;
    let issues = eoa_catalog::validate(&catalog);
    if !issues.is_empty() {
        let detail = issues
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        return Err(CommandError::new("catalog.invalid", detail));
    }
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
            let client = catalog
                .clients
                .get(&entry.manifest.client)
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
                id: entry.manifest.id.clone(),
                name: entry.manifest.name.clone(),
                description: entry.manifest.description.clone(),
                client,
                processes: entry
                    .manifest
                    .processes
                    .iter()
                    .map(|p| p.id.clone())
                    .collect(),
                modules,
            }
        })
        .collect())
}

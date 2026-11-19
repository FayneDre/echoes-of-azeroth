use std::collections::BTreeMap;

use crate::manifest::{
    ClientManifest, ComponentManifest, EmulatorManifest, FlavorManifest, ModuleManifest,
    ServerDataSource,
};

/// Everything an emulator family brings: its manifest, flavors and modules.
/// Flavor and module ids are unique *within* an emulator.
#[derive(Debug, Clone)]
pub struct EmulatorEntry {
    pub manifest: EmulatorManifest,
    pub flavors: BTreeMap<String, FlavorManifest>,
    pub modules: BTreeMap<String, ModuleManifest>,
}

/// In-memory catalog. Built by `rk-catalog` (from disk or the signed remote
/// index); consumed by the resolver, the install engine and the UI.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub emulators: BTreeMap<String, EmulatorEntry>,
    pub clients: BTreeMap<String, ClientManifest>,
    pub components: BTreeMap<String, ComponentManifest>,
}

impl Catalog {
    pub fn emulator(&self, id: &str) -> Option<&EmulatorEntry> {
        self.emulators.get(id)
    }

    pub fn flavor(&self, emulator: &str, id: &str) -> Option<&FlavorManifest> {
        self.emulators.get(emulator)?.flavors.get(id)
    }

    pub fn module(&self, emulator: &str, id: &str) -> Option<&ModuleManifest> {
        self.emulators.get(emulator)?.modules.get(id)
    }

    /// Where `emulator`'s server data comes from: its own extraction when it
    /// declares one (ADR 0024), else its client's download (ADR 0006).
    /// `None` for an unknown emulator or a catalog that gives neither
    /// (`rk-catalog` validation rejects that).
    pub fn server_data_source(&self, emulator: &str) -> Option<ServerDataSource<'_>> {
        let manifest = &self.emulators.get(emulator)?.manifest;
        if let Some(extraction) = &manifest.data_extraction {
            return Some(ServerDataSource::Extract(extraction));
        }
        self.clients
            .get(&manifest.client)?
            .server_data
            .as_ref()
            .map(ServerDataSource::Download)
    }

    /// Modules that can never be removed from an instance of `flavor`:
    /// the flavor's required modules plus everything they transitively require.
    pub fn mandatory_modules(&self, emulator: &str, flavor: &str) -> Vec<String> {
        let Some(flavor) = self.flavor(emulator, flavor) else {
            return Vec::new();
        };
        let mut stack: Vec<String> = flavor.required_modules.clone();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = stack.pop() {
            if seen.insert(id.clone()) {
                if let Some(module) = self.module(emulator, &id) {
                    stack.extend(module.requires.iter().cloned());
                }
            }
        }
        seen.into_iter().collect()
    }
}

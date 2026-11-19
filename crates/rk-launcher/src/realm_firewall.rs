//! The LAN firewall rule of a realm (ADR 0022).

use super::*;

impl Launcher {
    /// The server programs of a realm other computers connect to, with the
    /// TCP port each is waited on.
    fn firewall_programs(&self, record: &InstanceRecord) -> Result<Vec<firewall::Program>> {
        let emulator = self.emulator(&record.emulator)?;
        // Windows wants a full path with backslashes (the data root may be
        // relative, as the CLI's default is).
        let root = self.layout.instance(&record.id).root;
        let root = std::path::absolute(&root).map_err(|e| io_error(root.clone(), e))?;
        Ok(emulator
            .processes
            .iter()
            .map(|spec| firewall::Program {
                process: spec.id.clone(),
                path: std::path::absolute(root.join(&spec.executable))
                    .unwrap_or_else(|_| root.join(&spec.executable)),
                port: match &spec.readiness {
                    rk_core::manifest::Readiness::Tcp { port } => record.ports.get(port).copied(),
                    rk_core::manifest::Readiness::LogLine { .. } => None,
                },
            })
            .collect())
    }

    /// Whether the firewall lets the other computers of the network reach
    /// the realm's servers.
    pub async fn firewall_status(&self, id: &str) -> Result<firewall::FirewallStatus> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let programs = self.firewall_programs(&record)?;
        Ok(firewall::status(id, &programs).await)
    }

    /// Lets the other computers of the network reach the realm's servers:
    /// one firewall rule per server program (Windows asks for elevation).
    pub async fn allow_firewall(&self, id: &str) -> Result<firewall::FirewallStatus> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let programs = self.firewall_programs(&record)?;
        firewall::allow(id, &programs, &self.firewall_log()).await?;
        Ok(firewall::status(id, &programs).await)
    }

    /// Removes the realm's firewall rules (Windows asks for elevation).
    pub async fn remove_firewall(&self, id: &str) -> Result<firewall::FirewallStatus> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let programs = self.firewall_programs(&record)?;
        firewall::remove(id, &self.firewall_log()).await?;
        Ok(firewall::status(id, &programs).await)
    }

    /// Where the elevated firewall change writes its error, if any: a full
    /// path (the elevated PowerShell starts in another folder).
    pub(crate) fn firewall_log(&self) -> PathBuf {
        let path = self.layout.state().join("firewall-error.txt");
        std::path::absolute(&path).unwrap_or(path)
    }

    /// Removes the unpacked module assets (`assets/<sha256>/`) that no realm
    /// selects any more: options turned off, realms removed, older pins
    /// (ADR 0022). Nothing is removed while a realm is being installed or
    /// has an unfinished change (its plan may still need them). The
    /// archives stay in the store: turning an option on again unpacks them
    /// without a download. Server overlays and game folders hold copies,
    /// never links into `assets/`. Best effort; returns what went.
    pub fn prune_assets(&self) -> Vec<PathBuf> {
        let Some(keep) = self.assets_in_use() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(self.layout.assets()) else {
            return Vec::new();
        };
        let mut removed = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Only unpacked archives: a staging folder has a longer name.
            let unpacked = name.len() == 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !unpacked || keep.contains(&name) || !entry.path().is_dir() {
                continue;
            }
            if std::fs::remove_dir_all(entry.path()).is_ok() {
                removed.push(entry.path());
            }
        }
        removed
    }

    /// Archive hashes of the assets every realm selects; `None` when one
    /// is not fully installed or has a change in flight.
    fn assets_in_use(&self) -> Option<BTreeSet<String>> {
        let mut keep = BTreeSet::new();
        for summary in self.instances().ok()? {
            let record = &summary.record;
            if !summary.installed
                || summary.unfinished.is_some()
                || self.pending_path(&record.id).exists()
            {
                return None;
            }
            let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
            let resolution = rk_core::resolve(
                &self.catalog,
                &record.emulator,
                Some(&record.flavor),
                &selected,
            )
            .ok()?;
            let assets =
                install::assets::selected(&self.catalog, &resolution, &record.options).ok()?;
            keep.extend(assets.into_iter().map(|a| a.asset.source.sha256));
        }
        Some(keep)
    }

    /// Shared downloads and their journals, once no realm uses them. Kept:
    /// backups, the registered clients, the accepted index (anti-rollback).
    pub(crate) fn remove_shared(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let root = self.layout.instances();
        let root = root.parent().unwrap_or(&root);
        for name in ["components", "serverdata", "assets", "store"] {
            let path = root.join(name);
            match std::fs::remove_dir_all(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    warnings.push(format!("{}: {e}", path.display()))
                }
                _ => {}
            }
        }
        // Their operations must run again: forget them.
        if let Ok(entries) = std::fs::read_dir(self.layout.state()) {
            let engine = Engine::new(self.layout.state());
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(op) = name.strip_suffix(".journal.json") {
                    if let Err(e) = engine.forget(op) {
                        warnings.push(format!("{op}: {e}"));
                    }
                }
            }
        }
        // The open store keeps writing there: its folders come back empty.
        for dir in ["blobs", "partial"] {
            let path = self.layout.store().join(dir);
            if let Err(e) = std::fs::create_dir_all(&path) {
                warnings.push(format!("{}: {e}", path.display()));
            }
        }
        warnings
    }
}

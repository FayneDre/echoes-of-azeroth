//! Removing a realm (ADR 0017).

use super::*;

impl Launcher {
    /// Removes an installed realm (servers stopped). With `keep_backup`, the
    /// player data is backed up first: backups are never removed here, and
    /// installing the realm again lists them for a restore. Files the
    /// launcher put in the game folder for this realm are removed. Once no
    /// realm is left, the shared downloads go too (database engine, server
    /// data, module files, download cache): a new install fetches them again.
    pub async fn uninstall(&self, id: &str, keep_backup: bool) -> Result<UninstallReport> {
        let summary = self.instance(id)?;
        let record = summary.record;
        let mut warnings = Vec::new();
        let backup = {
            let _lock = install::lock_instance(&self.layout, id)?;
            let backup = if keep_backup && summary.installed && summary.unfinished.is_none() {
                Some(
                    self.dump_player_data(id, backups::manual_name(unix_secs()), false)
                        .await?,
                )
            } else {
                None
            };
            // The database may still run (a crash, or adopted, ADR 0011):
            // its files could not be deleted.
            if let Ok((_, secrets)) = install::open_existing(&self.layout, id) {
                if let Ok(plan) =
                    install::instance_plan(&self.catalog, &self.layout, &record, &secrets)
                {
                    if let Err(e) = plan.db.stop().await {
                        warnings.push(format!("database: {e}"));
                    }
                }
            }
            if let Err(e) = self.release_client(&record) {
                warnings.push(format!("game folder: {e}"));
            }
            // Its LAN firewall rules (ADR 0022) go too: only when there are
            // some, so Windows asks for elevation only if the player once
            // allowed this realm. Declined: they stay, said in a warning.
            if firewall::has_rules(id).await {
                if let Err(e) = firewall::remove(id, &self.firewall_log()).await {
                    warnings.push(format!("firewall rules of '{id}': {e}"));
                }
            }
            backup
        };

        // Out of the way first (one rename), then deleted: an interrupted
        // delete leaves a folder nobody reads, not a half instance.
        let dir = self.layout.instance(id).root;
        let trash = self
            .layout
            .instances()
            .join(format!(".removed-{id}-{}", unix_secs()));
        std::fs::rename(&dir, &trash).map_err(|e| io_error(dir.clone(), e))?;
        let engine = Engine::new(self.layout.state());
        for op in [
            install::instance_operation_id(id),
            install::modules_operation_id(id),
        ] {
            engine.forget(&op)?;
        }
        if let Err(e) = std::fs::remove_dir_all(&trash) {
            warnings.push(format!("{}: {e}", trash.display()));
        }
        self.remove_trash();

        let shared_removed = self.instances()?.is_empty();
        if shared_removed {
            warnings.extend(self.remove_shared());
        } else {
            self.prune_assets();
        }
        Ok(UninstallReport {
            backup,
            shared_removed,
            warnings,
        })
    }

    /// Removes the files the launcher put in the game folder for `record`
    /// (if the client was last prepared for it).
    fn release_client(&self, record: &InstanceRecord) -> Result<()> {
        let manifest = self.client_manifest_for(&record.emulator)?.clone();
        let dir = self.layout.clients();
        let _lock = rk_client::lock(&dir, &manifest.id)?;
        let Some(mut state) = rk_client::load(&dir, &manifest.id)? else {
            return Ok(());
        };
        if state.last_server.as_deref() == Some(record.id.as_str()) && !state.managed.is_empty() {
            rk_client::prepare(
                &dir,
                &manifest,
                &mut state,
                "127.0.0.1",
                &record.id,
                &[],
                &[],
            )?;
        }
        Ok(())
    }

    /// Leftovers of interrupted uninstalls and duplicates (operations run one
    /// at a time, so none of these is in use when this runs).
    pub(crate) fn remove_trash(&self) {
        let Ok(entries) = std::fs::read_dir(self.layout.instances()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".removed-") || name.starts_with(".copy-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

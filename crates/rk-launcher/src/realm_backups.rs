//! Backups and restores of a realm (ADR 0016, 0022).

use super::*;

impl Launcher {
    /// Folder of an instance's backups (may not exist yet).
    pub fn backups_dir(&self, id: &str) -> PathBuf {
        self.layout.backups(id)
    }

    /// Backups of an instance, newest first.
    pub fn backups(&self, id: &str) -> Result<Vec<backups::Backup>> {
        install::open_existing(&self.layout, id)?;
        backups::list(&self.layout.backups(id))
    }

    /// Dumps the player databases (characters, accounts, bots) to a new
    /// manual backup: what [`restore`](Self::restore) puts back. Content
    /// databases are left out, the build and modules rebuild them. The
    /// servers must be stopped (the instance lock): the database is
    /// started for the dump and stopped again. While they run, see
    /// [`Self::backup_live`].
    pub async fn backup_now(&self, id: &str) -> Result<backups::Backup> {
        let _lock = install::lock_instance(&self.layout, id)?;
        self.dump_player_data(id, backups::manual_name(unix_secs()), false)
            .await
    }

    /// The manual backup of a realm whose servers run (ADR 0022): their
    /// database is dumped in one transaction and left running. Only for
    /// work run by the realm's session
    /// ([`SessionControl::while_running`](session::SessionControl::while_running)),
    /// which holds the instance lock, has saved the players and keeps the
    /// database up until this returns.
    pub async fn backup_live(&self, id: &str) -> Result<backups::Backup> {
        self.dump_player_data(id, backups::manual_name(unix_secs()), true)
            .await
    }

    /// The daily backup (ADR 0018): the player databases, at most once a
    /// day, made when the servers have stopped. `None` when one is less
    /// than a day old or the realm is not fully installed. Busy when the
    /// realm runs or changes (instance lock).
    pub async fn daily_backup(&self, id: &str) -> Result<Option<backups::Backup>> {
        let dir = self.layout.backups(id);
        if !backups::daily_due(&dir, unix_secs())? {
            return Ok(None);
        }
        let summary = self.instance(id)?;
        if !summary.installed || summary.unfinished.is_some() {
            return Ok(None);
        }
        let _lock = install::lock_instance(&self.layout, id)?;
        let backup = self
            .dump_player_data(id, backups::daily_name(unix_secs()), false)
            .await?;
        // Best effort, like after an update.
        let _ = backups::prune(&dir);
        Ok(Some(backup))
    }

    /// Dumps the player databases to `backups/<id>/<name>`, the instance
    /// lock held by the caller, or by the running servers (`running`: their
    /// database is left as it is).
    pub(crate) async fn dump_player_data(
        &self,
        id: &str,
        name: String,
        running: bool,
    ) -> Result<backups::Backup> {
        let dir = self.layout.backups(id);
        std::fs::create_dir_all(&dir).map_err(|e| io_error(dir.clone(), e))?;
        self.dump_player_data_to(id, &dir.join(&name), running)
            .await?;
        backups::list(&dir)?
            .into_iter()
            .find(|b| b.name == name)
            .ok_or(LauncherError::BackupNotFound(name))
    }

    /// Dumps the player databases to `out`, the instance lock held by the
    /// caller (or by the running servers, `running`).
    pub(crate) async fn dump_player_data_to(
        &self,
        id: &str,
        out: &Path,
        running: bool,
    ) -> Result<()> {
        let (record, secrets) = install::open_existing(&self.layout, id)?;
        install::ensure_finished(
            &self.layout,
            &[
                install::instance_operation_id(id),
                install::modules_operation_id(id),
            ],
        )?;
        let plan = install::instance_plan(&self.catalog, &self.layout, &record, &secrets)?;
        let player = rk_runtime::modules::player_databases(&self.catalog, &record)?;
        let databases: Vec<&str> = player.iter().map(String::as_str).collect();
        if running {
            return Ok(plan.db.backup(&databases, out).await?);
        }
        plan.db.start().await?;
        let dumped = plan.db.backup(&databases, out).await;
        let stopped = plan.db.stop().await;
        dumped?;
        stopped?;
        Ok(())
    }

    /// Deletes one backup file. Also while the instance lock is held by
    /// something that does not read backups (running servers, an export),
    /// never while a change is in flight: a restore reads its backup until
    /// it completes (`pending.json`, operation journals).
    pub fn delete_backup(&self, id: &str, name: &str) -> Result<()> {
        install::open_existing(&self.layout, id)?;
        let _lock = match install::lock_instance(&self.layout, id) {
            Ok(lock) => Some(lock),
            Err(busy @ install::PlanError::InstanceBusy(_)) => {
                let operations = [
                    install::instance_operation_id(id),
                    install::modules_operation_id(id),
                ];
                if self.pending_path(id).exists()
                    || install::ensure_finished(&self.layout, &operations).is_err()
                {
                    return Err(busy.into());
                }
                None
            }
            Err(other) => return Err(other.into()),
        };
        let path = backups::resolve(&self.layout.backups(id), name)?;
        std::fs::remove_file(&path).map_err(|e| io_error(path, e))
    }

    /// Puts the player databases (characters, accounts, bots) back as the
    /// backup `name` holds them; the current state is dumped first. Content
    /// databases are kept, and dbimport migrates the result to the current
    /// build. Resumable like an update.
    pub async fn restore(
        &self,
        id: &str,
        name: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let summary = self.instance(id)?;
        // An interrupted change must be resumed first. A pending.json left
        // by a change that was rolled back is stale: replaced below, as
        // update and repair do.
        if !summary.installed || summary.unfinished.is_some() {
            return Err(install::PlanError::Unfinished(id.to_owned()).into());
        }
        let record = summary.record;
        let _lock = install::lock_instance(&self.layout, id)?;
        let dump = backups::resolve(&self.layout.backups(id), name)?;
        self.restore_locked(record, dump, name, observer, cancel)
            .await
    }

    /// [`Self::restore`] once checked, the instance lock held.
    pub(crate) async fn restore_locked(
        &self,
        record: InstanceRecord,
        dump: PathBuf,
        name: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let id = record.id.clone();
        let id = id.as_str();
        if !rk_db::dump_complete(&dump)? {
            return Err(LauncherError::BackupIncomplete(name.to_owned()));
        }
        let held = {
            let dump = dump.clone();
            tokio::task::spawn_blocking(move || rk_db::dump_databases(&dump))
                .await
                .map_err(|e| LauncherError::Invalid(e.to_string()))??
        };
        let databases: Vec<String> = rk_runtime::modules::player_databases(&self.catalog, &record)?
            .into_iter()
            .filter(|d| held.contains(d))
            .collect();
        if databases.is_empty() {
            return Err(LauncherError::BackupUnusable(name.to_owned()));
        }
        let pending = Pending::Restore {
            dump,
            databases,
            label: format!("restore-{}", unix_secs()),
        };
        self.save_pending(id, &pending)?;
        self.finish(record, pending, observer, cancel).await
    }
}

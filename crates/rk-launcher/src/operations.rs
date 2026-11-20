//! Install, update, repair, resume and module changes of a realm (ADR 0014).

use super::*;

impl Launcher {
    /// What a first install of `emulator` downloads and takes on the disk.
    /// Shared parts already installed (database engine, server data) count
    /// for nothing; the server build itself is checked when it downloads.
    pub fn install_space(&self, emulator: &str) -> Result<SpaceEstimate> {
        let emu = self.emulator(emulator)?;
        let platform = install::platform();
        let (mut download, mut needed) = (0u64, 0u64);
        let mut add = |sha256: &str, size: u64, unpacked: Option<u64>, target: PathBuf| {
            if target.is_dir() {
                return;
            }
            if !self.store.contains(sha256) {
                download += size;
                needed += size;
            }
            needed += unpacked.unwrap_or(size);
        };
        for component in &emu.components {
            if let Some(build) = self
                .catalog
                .components
                .get(&component.id)
                .and_then(|m| m.platforms.get(&platform))
            {
                add(
                    &build.sha256,
                    build.size,
                    build.unpacked_size,
                    self.layout.component(&component.id),
                );
            }
        }
        let source = self.catalog.server_data_source(emulator);
        if let (Some(client), Some(source)) = (self.catalog.clients.get(&emu.client), source) {
            let target = self
                .layout
                .server_data(&client.id, &source.folder(emulator));
            match source {
                ServerDataSource::Download(data) => {
                    add(&data.sha256, data.size, data.unpacked_size, target)
                }
                // Made on this machine: nothing to download, only its size.
                ServerDataSource::Extract(extraction) if !target.is_dir() => {
                    needed += extraction.unpacked_size.unwrap_or(0)
                }
                ServerDataSource::Extract(_) => {}
            }
        }
        needed += emu.instance.disk_size.unwrap_or(0);
        let available = rk_store::available_space(self.layout.root()).ok();
        Ok(SpaceEstimate {
            download,
            needed,
            available,
            too_small: available
                .is_some_and(|a| a < needed.saturating_add(rk_store::space::MARGIN)),
        })
    }

    /// Installs an instance (resumable: the same request again resumes).
    pub async fn install(
        &self,
        request: &InstallRequest,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let emu = self.emulator(&request.emulator)?.clone();
        if emu.experimental && !experimental_enabled() {
            return Err(LauncherError::Experimental(emu.id));
        }
        // A new realm: refuse before downloading gigabytes onto a disk that
        // cannot hold them. A resumed one already took part of its room.
        if !self
            .layout
            .instance(&request.id)
            .root
            .join("instance.json")
            .is_file()
        {
            location::check(self.layout.root())?;
            let space = self.install_space(&request.emulator)?;
            if let (true, Some(available)) = (space.too_small, space.available) {
                return Err(LauncherError::NoSpace(rk_store::NoSpace {
                    path: self.layout.root().to_path_buf(),
                    needed: space.needed.saturating_add(rk_store::space::MARGIN),
                    available,
                }));
            }
        }
        // Shared resources first, each in its own operation.
        for component in &emu.components {
            let op = install::component_operation(
                &self.catalog,
                &self.layout,
                self.store.clone(),
                &component.id,
            )?;
            self.run(&op, observer, cancel).await?;
        }
        // Downloaded data comes first; extracted data needs the realm's tools.
        if let Some((op, _)) = install::server_data_operation(
            &self.catalog,
            &self.layout,
            self.store.clone(),
            &request.emulator,
        )? {
            self.run(&op, observer, cancel).await?;
        }

        let new = install::NewInstance {
            id: request.id.clone(),
            emulator: request.emulator.clone(),
            flavor: request.flavor.clone(),
            modules: request.modules.clone(),
            network: request.network,
        };
        let (record, _) = install::open_or_create(&self.catalog, &self.layout, &new)?;
        let _lock = install::lock_instance(&self.layout, &record.id)?;
        let pending = match (&request.server, self.pending(&record.id)?) {
            (Some(server), _) => Pending::Install {
                server: server.clone(),
                db_snapshot: request.db_snapshot.clone(),
            },
            // A resumed install keeps its files (same plan) while they exist.
            (None, Some(pending @ Pending::Install { .. })) if pending.files_exist() => pending,
            (None, _) => {
                let (server, db_snapshot) = self
                    .fetch_build(&record, request.index.as_deref(), true, observer, cancel)
                    .await?;
                Pending::Install {
                    server,
                    db_snapshot,
                }
            }
        };
        self.save_pending(&record.id, &pending)?;
        let record = self.finish(record, pending, observer, cancel).await?;
        self.extract_server_data(&record, observer, cancel).await?;
        Ok(record)
    }

    /// Makes the server data of an emulator that extracts it from the
    /// player's client (ADR 0024), with the tools of `record`'s build; shared
    /// by every realm of the emulator, so it runs once. Nothing to do for
    /// downloaded data or when it is already there. Resumable.
    pub(crate) async fn extract_server_data(
        &self,
        record: &InstanceRecord,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<()> {
        let Some(ServerDataSource::Extract(_)) = self.catalog.server_data_source(&record.emulator)
        else {
            return Ok(());
        };
        let client = self.client_manifest_for(&record.emulator)?;
        let state = rk_client::load(&self.layout.clients(), &client.id)?
            .ok_or_else(|| LauncherError::NoClient(client.version.clone()))?;
        let paths = self.layout.instance(&record.id);
        let input = install::ExtractionInput {
            tools_root: paths.root.clone(),
            client_root: state.path,
            logs: paths.logs.join("extract"),
            cpus: std::thread::available_parallelism().map_or(1, |n| n.get()),
        };
        if let Some((op, target)) =
            install::extraction_operation(&self.catalog, &self.layout, &record.emulator, input)?
        {
            if !target.is_dir() {
                self.run(&op, observer, cancel).await?;
            }
        }
        Ok(())
    }

    /// Runs the `instance-<id>` operation described by `pending`, records
    /// the new build, then applies the modules (post-install steps, final
    /// configs). The instance lock is held by the caller.
    pub(crate) async fn finish(
        &self,
        record: InstanceRecord,
        pending: Pending,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let (_, secrets) = install::open_existing(&self.layout, &record.id)?;
        let driver = driver(&self.emulator(&record.emulator)?.driver)?;
        let server = match &pending {
            Pending::Install {
                server,
                db_snapshot,
            } => {
                let op = install::instance_operation(
                    &self.catalog,
                    &self.layout,
                    &record,
                    &secrets,
                    server,
                    db_snapshot.as_deref(),
                )?;
                self.run(&op, observer, cancel).await?;
                Some(server)
            }
            Pending::Restore {
                dump,
                databases,
                label,
            } => {
                let op = rk_runtime::modules::restore_operation(
                    &self.catalog,
                    &self.layout,
                    &record,
                    &secrets,
                    driver.clone(),
                    dump,
                    databases.clone(),
                    label,
                )?;
                self.run(&op, observer, cancel).await?;
                None
            }
            Pending::Update { server, label } => {
                let op = rk_runtime::modules::update_operation(
                    &self.catalog,
                    &self.layout,
                    &record,
                    &secrets,
                    driver.clone(),
                    &self.store,
                    server,
                    label,
                )?;
                self.run(&op, observer, cancel).await?;
                Some(server)
            }
        };
        let build = server.map(|s| install::artifact_build_key(s)).transpose()?;
        if matches!(pending, Pending::Install { .. }) {
            // Part of the install: until it completes, the instance is not
            // installed and `resume` runs both operations again (the first
            // one answers "already completed").
            let op = rk_runtime::modules::apply_modules_operation(
                &self.catalog,
                &self.layout,
                &record,
                &secrets,
                driver,
                &self.store,
            )?;
            self.run(&op, observer, cancel).await?;
        }
        let mut record = record;
        if let Some(build) = build {
            record.build = Some(build);
            install::save_record(&self.layout, &record)?;
        }
        self.clear_pending(&record.id)?;
        // Old automatic dumps go once a new one exists (never manual ones).
        // Best effort: the change itself succeeded, a locked file is
        // pruned next time.
        let _ = backups::prune(&self.layout.backups(&record.id));
        self.prune_assets();
        Ok(record)
    }

    /// The pending change with its files present: a store blob that was
    /// cleaned up is fetched again (same content, so the same path and plan).
    async fn with_files(
        &self,
        record: &InstanceRecord,
        pending: Pending,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<Pending> {
        if pending.files_exist() {
            return Ok(pending);
        }
        Ok(match pending {
            Pending::Install { db_snapshot, .. } => {
                let (server, snapshot) = self
                    .fetch_build(record, None, db_snapshot.is_some(), observer, cancel)
                    .await?;
                Pending::Install {
                    server,
                    db_snapshot: snapshot,
                }
            }
            Pending::Update { label, .. } => {
                let (server, _) = self
                    .fetch_build(record, None, false, observer, cancel)
                    .await?;
                Pending::Update { server, label }
            }
            Pending::Restore { dump, .. } => {
                return Err(LauncherError::Invalid(format!(
                    "the backup {} being restored was deleted",
                    dump.display()
                )))
            }
        })
    }

    /// Installs the server build of the current catalog release.
    pub async fn update(
        &self,
        id: &str,
        server: Option<&Path>,
        index: Option<&str>,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<UpdateOutcome> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let _lock = install::lock_instance(&self.layout, id)?;
        let from = record.build.clone();
        let server = match server {
            Some(server) => server.to_path_buf(),
            None => {
                self.fetch_build(&record, index, false, observer, cancel)
                    .await?
                    .0
            }
        };
        let to = install::artifact_build_key(&server)?;
        if from.as_deref() == Some(to.as_str()) {
            return Ok(UpdateOutcome::UpToDate { build: to });
        }
        let label = rk_runtime::modules::update_label(from.as_deref(), &to);
        let pending = Pending::Update { server, label };
        self.save_pending(id, &pending)?;
        self.finish(record, pending, observer, cancel).await?;
        Ok(UpdateOutcome::Updated { from, to })
    }

    /// Reinstalls the server files, SQL tree and configs of the current
    /// release and runs the database update, like an update to the same
    /// build (databases dumped first, rolled back on failure). Fixes deleted
    /// or edited server files; player data is untouched.
    pub async fn repair(
        &self,
        id: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let _lock = install::lock_instance(&self.layout, id)?;
        let (server, _) = self
            .fetch_build(&record, None, false, observer, cancel)
            .await?;
        let to = install::artifact_build_key(&server)?;
        let label = format!("repair-{}-{}", &to[..to.len().min(16)], unix_secs());
        let pending = Pending::Update { server, label };
        self.save_pending(id, &pending)?;
        self.finish(record, pending, observer, cancel).await
    }

    /// Resumes the interrupted change of an instance (install, update,
    /// repair, module or option change, import) with the plan it started with.
    pub async fn resume(
        &self,
        id: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let record = self.resume_change(id, observer, cancel).await?;
        if self.import_unfinished(id) {
            return self.finish_import(id, observer, cancel).await;
        }
        self.extract_server_data(&record, observer, cancel).await?;
        Ok(record)
    }

    async fn resume_change(
        &self,
        id: &str,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let summary = self.instance(id)?;
        let record = summary.record;
        let _lock = install::lock_instance(&self.layout, id)?;
        // An install, update or repair in progress: run it to its end with
        // the inputs it started with (its module operation included).
        if let Some(pending) = self.pending(id)? {
            let pending = self.with_files(&record, pending, observer, cancel).await?;
            self.save_pending(id, &pending)?;
            return self.finish(record, pending, observer, cancel).await;
        }
        let import_op = format!("import-{id}");
        match summary.unfinished.as_deref() {
            Some(op) if op == install::modules_operation_id(id) => {
                self.apply(record.clone(), &record, observer, cancel).await
            }
            // The data root was renamed or moved: the module operation
            // writes every config, the server data overlay and the module
            // files again at the new place (its journal is forgotten: the
            // plan may look the same).
            None if summary.moved => {
                Engine::new(self.layout.state()).forget(&install::modules_operation_id(id))?;
                self.apply(record.clone(), &record, observer, cancel).await
            }
            // Finished by `resume` after this (`import.json`).
            Some(op) if op == import_op && summary.installed => Ok(record),
            Some(op) if op != import_op => Err(LauncherError::Invalid(format!(
                "no record of the interrupted change of '{id}'"
            ))),
            // Rolled back before it completed, without its inputs (made by
            // an older launcher), or an import whose install stopped after
            // its record but before its plan (no matching build in the index
            // yet, failed download): install again from the index.
            _ if !summary.installed => {
                let (server, db_snapshot) = self
                    .fetch_build(&record, None, true, observer, cancel)
                    .await?;
                let pending = Pending::Install {
                    server,
                    db_snapshot,
                };
                self.save_pending(id, &pending)?;
                self.finish(record, pending, observer, cancel).await
            }
            _ => Ok(record),
        }
    }

    /// Runs `modules-<id>` for a record just saved; puts `old` back if it fails.
    pub(crate) async fn apply(
        &self,
        record: InstanceRecord,
        old: &InstanceRecord,
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let (_, secrets) = install::open_existing(&self.layout, &record.id)?;
        let emu = self.emulator(&record.emulator)?;
        let op = rk_runtime::modules::apply_modules_operation(
            &self.catalog,
            &self.layout,
            &record,
            &secrets,
            driver(&emu.driver)?,
            &self.store,
        )?;
        if let Err(error) = self.run(&op, observer, cancel).await {
            install::save_record(&self.layout, old)?;
            return Err(error);
        }
        let record = rk_runtime::modules::finished(&self.layout, &record)?;
        self.prune_assets();
        Ok(record)
    }

    /// Turns modules on or off (servers stopped). A removed module that
    /// changed the world rebuilds it (ADR 0012): `regenerate` in the
    /// returned record tells, before it starts, via [`Self::plan_modules`].
    pub async fn set_modules(
        &self,
        id: &str,
        enable: &[String],
        disable: &[String],
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let _lock = install::lock_instance(&self.layout, id)?;
        let (record, old) =
            rk_runtime::modules::set_modules(&self.catalog, &self.layout, id, enable, disable)?;
        self.apply(record, &old, observer, cancel).await
    }

    /// Content databases that disabling `disable` would rebuild (to warn
    /// the player that it takes minutes). Changes nothing.
    pub fn rebuilds_for(&self, id: &str, disable: &[String]) -> Result<Vec<String>> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let entry = self
            .catalog
            .emulator(&record.emulator)
            .ok_or_else(|| LauncherError::UnknownEmulator(record.emulator.clone()))?;
        let mut out = Vec::new();
        for module in disable {
            let Some(manifest) = entry.modules.get(module) else {
                continue;
            };
            if !record.modules.contains(module) || manifest.database.apply_when_disabled {
                continue;
            }
            for db in &manifest.database.touches {
                let content = entry
                    .manifest
                    .databases
                    .iter()
                    .any(|d| &d.id == db && !d.player_data);
                if content && !out.contains(db) {
                    out.push(db.clone());
                }
            }
        }
        Ok(out)
    }

    /// Sets option values of one module (`(option, value text)`), applied
    /// like a module change.
    pub async fn set_options(
        &self,
        id: &str,
        module: &str,
        values: &[(String, String)],
        observer: &dyn Observer,
        cancel: &CancelToken,
    ) -> Result<InstanceRecord> {
        let _lock = install::lock_instance(&self.layout, id)?;
        let (record, old) =
            rk_runtime::modules::set_options(&self.catalog, &self.layout, id, module, values)?;
        self.apply(record, &old, observer, cancel).await
    }

    /// The options of the instance's modules, with the values in force.
    pub fn options(&self, record: &InstanceRecord) -> Vec<ModuleOptions> {
        let Some(entry) = self.catalog.emulator(&record.emulator) else {
            return Vec::new();
        };
        // Mandatory modules have options too (Playerbots).
        let modules = rk_runtime::modules::enabled_modules(&self.catalog, record)
            .unwrap_or_else(|_| record.modules.clone());
        modules
            .iter()
            .filter_map(|module| {
                let manifest = entry.modules.get(module)?;
                let chosen = record.options.get(module);
                let options: Vec<OptionView> = manifest
                    .options
                    .iter()
                    .map(|option| OptionView {
                        value: option.effective(chosen.and_then(|c| c.get(option.id()))),
                        option: option.clone(),
                    })
                    .collect();
                (!options.is_empty()).then(|| ModuleOptions {
                    module: module.clone(),
                    options,
                })
            })
            .collect()
    }

    /// The realm settings (rates, realm type...) with the values in force;
    /// changed with [`Self::set_options`] and the module id
    /// [`rk_core::manifest::REALM_OPTIONS`].
    pub fn realm_settings(&self, record: &InstanceRecord) -> Vec<SettingsView> {
        let Some(entry) = self.catalog.emulator(&record.emulator) else {
            return Vec::new();
        };
        let chosen = record.options.get(rk_core::manifest::REALM_OPTIONS);
        entry
            .manifest
            .settings
            .iter()
            .map(|section| SettingsView {
                id: section.id.clone(),
                label: section.label.clone(),
                options: section
                    .options
                    .iter()
                    .map(|option| OptionView {
                        value: option.effective(chosen.and_then(|c| c.get(option.id()))),
                        option: option.clone(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// The servers of an instance, ready to start. Refused while its
    /// folder was moved and its configs still name the old place, or when
    /// programs it needs are gone (an antivirus quarantine, ADR 0020).
    pub fn server(&self, id: &str) -> Result<rk_runtime::Server> {
        if configs_point_elsewhere(&self.layout.instance(id)) {
            return Err(LauncherError::Moved(id.to_owned()));
        }
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let missing = self.missing_binaries(server.record())?;
        if !missing.is_empty() {
            return Err(LauncherError::FilesMissing {
                instance: id.to_owned(),
                files: missing,
            });
        }
        Ok(server)
    }

    /// Programs and libraries of an instance (its server files, its
    /// components) that are gone or changed since they were installed.
    /// Folders unpacked by an older launcher have no list: their manifest
    /// executables are checked.
    fn missing_binaries(&self, record: &InstanceRecord) -> Result<Vec<String>> {
        let emu = self.emulator(&record.emulator)?;
        let paths = self.layout.instance(&record.id);
        let mut missing = Vec::new();
        let mut check = |dir: &Path, fallback: Vec<PathBuf>| {
            let shown = |name: &str| native(&dir.join(name));
            match rk_engine::binaries::missing(dir) {
                Some(names) => missing.extend(names.iter().map(|n| shown(n))),
                None => missing.extend(
                    fallback
                        .into_iter()
                        .filter(|p| !p.is_file())
                        .map(|p| native(&p)),
                ),
            }
        };
        let executables = emu
            .processes
            .iter()
            .map(|p| paths.root.join(&p.executable))
            .collect();
        check(&paths.bin, executables);
        for component in &emu.components {
            check(&self.layout.component(&component.id), Vec::new());
        }
        Ok(missing)
    }
}

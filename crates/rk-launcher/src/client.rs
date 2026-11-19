//! The player's game client: registration, preparation, LAN join (ADR 0013, 0016).

use super::*;

impl Launcher {
    /// The client manifest an emulator needs.
    pub fn client_manifest_for(&self, emulator: &str) -> Result<&ClientManifest> {
        let emu = self.emulator(emulator)?;
        self.catalog
            .clients
            .get(&emu.client)
            .ok_or_else(|| LauncherError::UnknownEmulator(emu.client.clone()))
    }

    pub fn client(&self, client: &str) -> Result<Option<ClientState>> {
        Ok(rk_client::load(&self.layout.clients(), client)?)
    }

    /// Registers the game folder for a client manifest; returns a warning
    /// when a previous folder keeps files the launcher put there.
    pub fn register_client(
        &self,
        client: &str,
        folder: &Path,
        locale: Option<&str>,
    ) -> Result<(ClientState, Option<String>)> {
        let manifest = self
            .catalog
            .clients
            .get(client)
            .ok_or_else(|| LauncherError::UnknownEmulator(client.to_owned()))?;
        let dir = self.layout.clients();
        let _lock = rk_client::lock(&dir, client)?;
        Ok(rk_client::register(&dir, manifest, folder, locale)?)
    }

    /// Points the registered client at an instance and syncs the
    /// instance's client files. The client stays locked by the result.
    pub fn prepare_client(&self, record: &InstanceRecord) -> Result<PreparedClient> {
        let manifest = self.client_manifest_for(&record.emulator)?.clone();
        // A half-applied change would give the client the wrong files.
        install::ensure_finished(
            &self.layout,
            &[
                install::instance_operation_id(&record.id),
                install::modules_operation_id(&record.id),
            ],
        )?;
        let dir = self.layout.clients();
        let lock = rk_client::lock(&dir, &manifest.id)?;
        let mut state = rk_client::load(&dir, &manifest.id)?
            .ok_or_else(|| LauncherError::NoClient(manifest.version.clone()))?;
        let selected: Vec<&str> = record.modules.iter().map(String::as_str).collect();
        let resolution = rk_core::resolve(
            &self.catalog,
            &record.emulator,
            Some(&record.flavor),
            &selected,
        )
        .map_err(install::PlanError::from)?;
        let assets = install::assets::selected(&self.catalog, &resolution, &record.options)?;
        let files = install::assets::client_files(&self.layout, &assets);
        if let Some((missing, _)) = files.iter().find(|(src, _)| !src.is_file()) {
            return Err(LauncherError::MissingAsset(missing.clone()));
        }
        let addons = install::assets::addon_files(&self.layout, &assets);
        // A selected add-on whose folder is gone or empty: apply again.
        for asset in &assets {
            for addon in &asset.asset.addons {
                let folder = asset.dir(&self.layout).join(&addon.from);
                if !folder.is_dir() {
                    return Err(LauncherError::MissingAsset(folder));
                }
            }
        }
        // The local client always reaches the servers through the loopback.
        let report = rk_client::prepare(
            &dir,
            &manifest,
            &mut state,
            "127.0.0.1",
            &record.id,
            &files,
            &addons,
        )?;
        Ok(PreparedClient {
            manifest,
            state,
            report,
            lock,
        })
    }

    /// Points the registered client of `emulator` at a realm hosted on
    /// another computer of the local network (`address`: IPv4 or host
    /// name). The host's module files are not known here, so the files the
    /// launcher put in the client are removed: a realm with client patches
    /// needs them copied by hand.
    pub fn prepare_client_for_lan(&self, emulator: &str, address: &str) -> Result<PreparedClient> {
        let address = address.trim();
        let valid = !address.is_empty()
            && address.len() <= 253
            && address
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        if !valid {
            return Err(LauncherError::InvalidAddress(address.to_owned()));
        }
        let manifest = self.client_manifest_for(emulator)?.clone();
        let dir = self.layout.clients();
        let lock = rk_client::lock(&dir, &manifest.id)?;
        let mut state = rk_client::load(&dir, &manifest.id)?
            .ok_or_else(|| LauncherError::NoClient(manifest.version.clone()))?;
        let server = format!("lan-{address}");
        let report = rk_client::prepare(&dir, &manifest, &mut state, address, &server, &[], &[])?;
        Ok(PreparedClient {
            manifest,
            state,
            report,
            lock,
        })
    }
}

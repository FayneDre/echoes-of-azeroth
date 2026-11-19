//! Game accounts of a realm.

use super::*;

impl Launcher {
    pub async fn create_account(&self, id: &str, account: &rk_runtime::NewAccount) -> Result<u32> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.create_account(driver.as_ref(), account).await?)
    }

    /// Player accounts of an instance (its database started for it if the
    /// servers are stopped).
    pub async fn accounts(&self, id: &str) -> Result<Vec<rk_runtime::AccountInfo>> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.accounts(driver.as_ref()).await?.unwrap_or_default())
    }

    /// Changes an account's GM level (applies at its next login).
    pub async fn set_gm_level(&self, id: &str, username: &str, gm_level: u8) -> Result<()> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server
            .set_gm_level(driver.as_ref(), username, gm_level)
            .await?)
    }

    /// Characters in the world (players first), at most `limit`; `None`
    /// while the servers are stopped or when the emulator cannot tell.
    pub async fn online_characters(
        &self,
        id: &str,
        limit: u32,
    ) -> Result<Option<Vec<rk_runtime::OnlineCharacter>>> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.online_characters(driver.as_ref(), limit).await?)
    }

    /// Named teleport places (empty while the servers are stopped).
    pub async fn teleport_locations(&self, id: &str) -> Result<Vec<String>> {
        let server = rk_runtime::Server::open(&self.catalog, &self.layout, id)?;
        let driver = driver(server.driver_id())?;
        Ok(server.teleport_locations(driver.as_ref()).await?)
    }

    /// The console line of a GM panel action, values checked (ADR 0020).
    pub fn console_action_line(
        &self,
        id: &str,
        action: &str,
        values: &std::collections::BTreeMap<String, String>,
    ) -> Result<String> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let emu = self.emulator(&record.emulator)?;
        let action = emu
            .console_actions
            .iter()
            .find(|a| a.id == action)
            .ok_or_else(|| LauncherError::Invalid(format!("unknown console action '{action}'")))?;
        Ok(rk_core::console::render(action, values)?)
    }
}

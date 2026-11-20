//! A party of bots formed for a player in the world (ADR 0025): checked
//! console lines of the realm's party module, sent through the session that
//! runs the servers, and their `RKPARTY` replies read back.

use std::time::Duration;

use rk_core::manifest::Party;
use rk_core::party::{self, Member, PartyError};

use super::*;
use crate::session::SessionControl;

/// The module answers at once; the bot itself joins later.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

impl Launcher {
    /// The party the realm's server offers: `None` when its build has no
    /// party module (installed before it existed: a server update adds it).
    /// The module is usually required by the flavor, so it is looked for
    /// among every running module, not only the ones the player chose; and
    /// it must be in the realm's build (its `.conf.dist` is there), which
    /// may be older than the catalog.
    pub fn party(&self, id: &str) -> Result<Option<Party>> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        let dist = self.layout.instance(id).bin.join("configs");
        let modules = rk_runtime::modules::enabled_modules(&self.catalog, &record)?;
        Ok(modules.iter().find_map(|module| {
            let manifest = self.catalog.module(&record.emulator, module)?;
            let built = manifest
                .config
                .files
                .iter()
                .all(|file| dist.join(&file.dist).is_file());
            manifest.party.clone().filter(|_| built)
        }))
    }

    fn offered_party(&self, id: &str) -> Result<Party> {
        self.party(id)?
            .ok_or_else(|| PartyError::Refused("disabled".into()).into())
    }

    /// Adds a bot of `class` (launcher class id) with premade spec `spec`
    /// at `level` to `player`'s party; returns the bot's name.
    pub async fn party_add(
        &self,
        control: &SessionControl,
        id: &str,
        player: &str,
        class: &str,
        spec: u32,
        level: u32,
    ) -> Result<String> {
        let party = self.offered_party(id)?;
        let line = party::add_line(&party, player, class, spec, level)?;
        Ok(party::added(&ask(control, &line).await?)?)
    }

    /// Sends `player`'s bots away; returns their names.
    pub async fn party_remove(
        &self,
        control: &SessionControl,
        id: &str,
        player: &str,
    ) -> Result<Vec<String>> {
        let party = self.offered_party(id)?;
        let line = party::remove_line(&party, player)?;
        Ok(party::removed(&ask(control, &line).await?)?)
    }

    /// `player`'s bots, set up or still joining.
    pub async fn party_members(
        &self,
        control: &SessionControl,
        id: &str,
        player: &str,
    ) -> Result<Vec<Member>> {
        let party = self.offered_party(id)?;
        let line = party::list_line(&party, player)?;
        Ok(party::members(&party, &ask(control, &line).await?)?)
    }
}

async fn ask(control: &SessionControl, line: &str) -> Result<Vec<String>> {
    control
        .ask(line, party::PREFIX, party::ends_answer, ANSWER_TIMEOUT)
        .await
        .ok_or_else(|| PartyError::NoAnswer.into())
}

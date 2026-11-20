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
    pub fn party(&self, id: &str) -> Result<Option<Party>> {
        let (record, _) = install::open_existing(&self.layout, id)?;
        Ok(record.modules.iter().find_map(|module| {
            self.catalog
                .module(&record.emulator, module)
                .and_then(|m| m.party.clone())
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

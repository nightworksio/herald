//! A release the forge deleted taken out of the group it was gathered
//! into (ADR-0048).

use herald_core::{Identity, Key, Pending, Step, group_identity};
use herald_ports::{Clock, Http, Ledger, Signer, Store, Versioned, Waiting, Written};

use super::{Grouped, raced};
use crate::announce::{Outcome, stopped, stored};

impl<S: Store, C: Clock, G: Signer, H: Http> Grouped<'_, '_, S, C, G, H> {
    /// The release `identity` taken out of the group `key`'s record, and
    /// the group's post brought to the members left, or withdrawn where
    /// none is left; the steps that landed, or what stopped it.
    pub(crate) async fn without(
        &self,
        key: &str,
        identity: &Identity,
    ) -> Result<Vec<Step>, Outcome> {
        let hold = self.hold(key);
        let waiting = self.run.ports.store.waiting();
        for _ in 0..self.config.announce.latest_attempts {
            let Some(kept) = waiting
                .pending(&hold)
                .await
                .map_err(|error| stopped(stored(error)))?
            else {
                return Ok(Vec::new());
            };
            let mut left = kept.value.clone();
            left.events.retain(|held| held.event.identity() != identity);
            if left.events.len() == kept.value.events.len() {
                return self.regathered(key, kept).await;
            }
            match waiting
                .put_pending(&left, Some(kept.version))
                .await
                .map_err(|error| stopped(stored(error)))?
            {
                Written::Written(version) => {
                    return self
                        .regathered(
                            key,
                            Versioned {
                                value: left,
                                version,
                            },
                        )
                        .await;
                }
                Written::Conflict => {}
            }
        }
        Err(stopped(raced(key)))
    }

    /// The group `key`'s post, where it was announced, brought to the
    /// members its record `kept` holds, or withdrawn where it holds none.
    async fn regathered(&self, key: &str, kept: Versioned<Pending>) -> Result<Vec<Step>, Outcome> {
        let group = Key::derive(self.route(), &group_identity(key));
        let posted = self
            .run
            .ports
            .store
            .ledger()
            .entry(&group)
            .await
            .map_err(|error| stopped(stored(error)))?;
        let Some(posted) = posted.filter(|posted| posted.value.withdrawn.is_none()) else {
            return Ok(Vec::new());
        };
        let Some(lead) = kept.value.events.first().map(|held| held.event.clone()) else {
            return self.run.erased(posted).await;
        };
        match self.flush(key, kept, &lead).await {
            Outcome::Sent { steps } => Ok(steps),
            Outcome::AlreadyAnnounced => Ok(Vec::new()),
            outcome => Err(outcome),
        }
    }
}

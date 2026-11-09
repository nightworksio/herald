use herald_core::DestinationName;
use herald_kinds::{Connect, Effect, Replying, Session};
use herald_ports::Response;

use crate::interact::{Interacting, took};
use crate::interactions::Via;

/// A sync of the destination's homeserver, polled as long as it is held.
pub(crate) struct Syncing<'a> {
    interacting: &'a Interacting,
    since: Option<String>,
}

impl<'a> Syncing<'a> {
    /// The sync `interacting` holds, where it carries commands by syncing.
    pub(crate) fn of(interacting: &'a Interacting) -> Option<Self> {
        (interacting.via() == Via::Sync).then_some(Self {
            interacting,
            since: None,
        })
    }
}

impl Session for Syncing<'_> {
    fn connect(&mut self) -> Connect {
        let wait = if self.since.is_some() {
            self.interacting.sync_wait()
        } else {
            0
        };
        Connect::Poll(self.interacting.sync(self.since.as_deref(), wait))
    }

    fn polled(
        &mut self,
        destination: &DestinationName,
        response: &Response,
    ) -> Result<Vec<Effect>, String> {
        took(response)?;
        let synced = self
            .interacting
            .synced(destination, &response.body)
            .map_err(|unread| unread.to_string())?;
        let mut effects = vec![Effect::Ready];
        if self.since.is_some() {
            effects.extend(
                synced
                    .asked
                    .into_iter()
                    .map(|(asked, replying)| Effect::Asked(asked, Replying::new(replying))),
            );
        }
        self.since = Some(synced.next);
        Ok(effects)
    }

    fn unreachable(&self) -> &'static str {
        "the homeserver could not be synced"
    }
}

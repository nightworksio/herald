use std::any::Any;

use herald_core::{Answering, Profile, SecretRef, Symbols, WaitOptions};
use herald_kinds::{Carried, DestinationConfig, Placed, Placement, Reached, same};
use herald_ports::Secret;

use crate::discord::{self, Credential, Discord};
use crate::interactions::{Interactions, Via};
use crate::options::{DestinationOptions, RouteOptions};

/// The kind of a Discord destination, as the config spells it.
pub const KIND: &str = "discord";

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn symbols_written(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        self.token.as_ref().or(self.webhook.as_ref())
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        self.token.iter().chain(&self.webhook).collect()
    }

    fn answering(&self) -> Option<Answering> {
        self.interactions.as_ref().map(Interactions::answering)
    }

    fn session_domains(&self) -> Vec<&str> {
        self.interactions
            .as_ref()
            .filter(|interactions| interactions.via == Via::Gateway)
            .map(Interactions::session_domains)
            .unwrap_or_default()
    }

    fn carried(&self) -> Option<Carried> {
        self.interactions.as_ref().map(|interactions| Carried {
            posted: interactions.via == Via::Http,
            what: "interactions Discord posts",
            session: "the gateway with via = \"gateway\"",
        })
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        let placed = placed.of::<RouteOptions>()?;
        Some((
            discord::profile(self.token.is_some(), placed.forum),
            Some(discord::profile(true, placed.forum)),
        ))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        let placed = placed.of::<RouteOptions>()?;
        let credential = if self.token.is_some() {
            Credential::Bot(secret)
        } else {
            Credential::Webhook(secret)
        };
        Some(Reached::new(
            Discord::new(credential, placed.clone(), self.api.as_deref())
                .reading_back(self.read_back.unwrap_or_default())
                .waiting(self.waits())
                .looking(self.look())
                .worded(
                    self.interactions
                        .as_ref()
                        .map(Interactions::wording)
                        .unwrap_or_default(),
                ),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

impl Placement for RouteOptions {
    fn threads_each_post(&self) -> bool {
        self.forum
    }

    fn place(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    fn opt_in_role(&self) -> Option<String> {
        self.subscribe
            .as_ref()
            .map(|subscribe| subscribe.role.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

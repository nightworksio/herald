use std::any::Any;

use herald_core::{Answering, Profile, SecretRef, Symbols, WaitOptions};
use herald_kinds::{
    Carried, DestinationConfig, DestinationKind, Fault, Placed, Placement, Placing, Reached, same,
};
use herald_ports::Secret;

use crate::discord::{self, Credential, Discord};
use crate::interactions::{Interactions, Subscribe, Via};
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

/// Discord, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscordKind {}

impl DestinationKind for DiscordKind {
    const NAME: &'static str = KIND;
    const DESCRIPTION: &'static str = "Discord, by bot token or webhook.";
    const LABEL: &'static str = "Discord";
    const ROUTE: &'static str = "DiscordRoute";
    const ROUTE_DESCRIPTION: &'static str = "A route to a Discord destination.";

    type Options = DestinationOptions;
    type Route = RouteOptions;

    fn checked(
        options: DestinationOptions,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(DestinationOptions, Vec<Fault>), Fault> {
        let bot = options.bot().map_err(Fault::here)?;
        let faults = options
            .interactions
            .as_ref()
            .and_then(|interactions| {
                interactions
                    .check()
                    .and_then(|()| {
                        if interactions.via == Via::Gateway && !bot {
                            Err("the gateway is joined as a bot; give the destination a token in place of its webhook".to_owned())
                        } else {
                            Ok(())
                        }
                    })
                    .err()
            })
            .map(|message| Fault::at("interactions", message))
            .into_iter()
            .collect();
        Ok((options, faults))
    }

    fn placed(
        options: &DestinationOptions,
        route: &RouteOptions,
        placing: Placing<'_>,
    ) -> Vec<Fault> {
        let mut faults: Vec<Fault> = route.check().err().map(Fault::here).into_iter().collect();
        if options.token.is_some() && route.channel.is_none() {
            faults.push(Fault::here(
                "a Discord bot posts to a channel; give channel = \"<the channel's id>\"",
            ));
        }
        if let Some(subscribe) = &route.subscribe
            && let Err(message) = subscribed(options, subscribe, placing)
        {
            faults.push(Fault::at("subscribe", message));
        }
        faults
    }
}

fn subscribed(
    options: &DestinationOptions,
    subscribe: &Subscribe,
    placing: Placing<'_>,
) -> Result<(), String> {
    subscribe.check()?;
    if options.token.is_none() {
        return Err(
            "a member is given the opt-in role by a bot; give the destination a token in place of its webhook".to_owned(),
        );
    }
    placing.answers_subscribing(options.answering())
}

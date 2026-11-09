use std::any::Any;

use herald_core::{Answering, Profile, SecretRef, Symbols, WaitOptions};
use herald_kinds::{
    Carried, DestinationConfig, DestinationKind, Fault, Placed, Placement, Placing, Reached, same,
};
use herald_ports::Secret;

use crate::interactions::{Interactions, Via};
use crate::options::{DestinationOptions, RouteOptions};
use crate::slack::{self, Credential, Slack};

/// The kind of a Slack destination, as the config spells it.
pub const KIND: &str = "slack";

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
        let interactions = self.interactions.as_ref();
        self.token
            .iter()
            .chain(&self.webhook)
            .chain(interactions.and_then(|interactions| interactions.signing.as_ref()))
            .chain(interactions.and_then(|interactions| interactions.app.as_ref()))
            .collect()
    }

    fn answering(&self) -> Option<Answering> {
        self.interactions.as_ref().map(Interactions::answering)
    }

    fn session_domains(&self) -> Vec<&str> {
        self.interactions
            .as_ref()
            .filter(|interactions| interactions.via == Via::Socket)
            .map(|interactions| interactions.session_domains(self.domain.unwrap_or_default()))
            .unwrap_or_default()
    }

    fn carried(&self) -> Option<Carried> {
        self.interactions.as_ref().map(|interactions| Carried {
            posted: interactions.via == Via::Http,
            what: "commands Slack posts",
            session: "Socket Mode with via = \"socket\"",
        })
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<RouteOptions>()?;
        Some((
            slack::profile(self.token.is_some()),
            Some(slack::profile(true)),
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
            Slack::new(credential, placed.clone(), self.api.as_deref())
                .on(self.domain.unwrap_or_default())
                .in_team(self.team.as_deref())
                .reading_back(self.read_back.unwrap_or_default())
                .waiting(self.waits())
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
    fn place(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

/// Slack, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlackKind {}

impl DestinationKind for SlackKind {
    const NAME: &'static str = KIND;
    const DESCRIPTION: &'static str = "Slack, by bot token or webhook.";
    const LABEL: &'static str = "Slack";
    const ROUTE: &'static str = "SlackRoute";
    const ROUTE_DESCRIPTION: &'static str = "A route to a Slack destination.";

    type Options = DestinationOptions;
    type Route = RouteOptions;

    fn checked(
        options: DestinationOptions,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(DestinationOptions, Vec<Fault>), Fault> {
        options.bot().map_err(Fault::here)?;
        options.team().map_err(Fault::here)?;
        let faults = options
            .interactions
            .as_ref()
            .and_then(|interactions| interactions.check().err())
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
                "a Slack bot posts to a channel; give channel = \"<the channel's id>\"",
            ));
        }
        if route.subscribe.is_some()
            && let Err(message) = subscribed(options, placing)
        {
            faults.push(Fault::at("subscribe", message));
        }
        faults
    }
}

fn subscribed(options: &DestinationOptions, placing: Placing<'_>) -> Result<(), String> {
    if !placing.subscribers {
        return Err(
            "a member subscribes to be mentioned; add mentions = { subscribers = true } to the route".to_owned(),
        );
    }
    placing.answers_subscribing(options.answering())
}

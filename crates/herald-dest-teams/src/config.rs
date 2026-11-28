use herald_core::{Answering, Delivery, Profile, SecretRef, Symbols, WaitOptions, Wording};
use herald_kinds::{
    Carried, Checking, DestinationConfig, DestinationKind, Fault, Interacted, Placed, Placement,
    Placing, Reached,
};
use herald_ports::{Boxed, DynSecrets, Secret, SecretError};

use crate::bot::Bot;
use crate::interact::Interacting;
use crate::interactions::Interactions;
use crate::options::{DestinationOptions, RouteOptions};
use crate::teams::{self, TEAMS, Teams, UNCHECKED};

/// The kind of a Microsoft Teams destination, as the config spells it.
pub const KIND: &str = "teams";

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
        self.webhook.as_ref().or(self.app_password.as_ref())
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        self.webhook.iter().chain(&self.app_password).collect()
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        let route = placed.of::<RouteOptions>()?;
        let threads = route.threads.unwrap_or_default();
        let reached = if self.webhook.is_some() {
            teams::profile()
        } else {
            teams::bot_profile(threads)
        };
        Some((reached, Some(teams::bot_profile(threads))))
    }

    fn public(&self, _: &Placed, _: &Delivery) -> Option<&'static str> {
        None
    }

    fn checking(&self, _: &Placed) -> Checking {
        Checking::Unchecked(UNCHECKED)
    }

    fn recalled(&self, written: &str) -> Option<Placed> {
        serde_json::from_str::<RouteOptions>(written)
            .ok()
            .filter(|route| route.unwritten().is_none())
            .map(Placed::new)
    }

    fn answering(&self) -> Option<Answering> {
        self.interactions.as_ref().map(Interactions::answering)
    }

    fn carried(&self) -> Option<Carried> {
        self.interactions.as_ref().map(|_| Carried {
            posted: true,
            what: "activities Teams posts to the bot",
            session: None,
        })
    }

    fn interacts<'a>(
        &'a self,
        credential: &'a Secret,
        _: &'a dyn DynSecrets,
    ) -> Boxed<'a, Result<Option<Interacted>, SecretError>> {
        let interacting = self
            .interactions
            .as_ref()
            .zip(self.registered().ok().flatten())
            .and_then(|(interactions, registered)| {
                Interacting::new(registered.app_id, credential.clone(), registered.hosts).map(
                    |interacting| {
                        interacting
                            .taking(interactions.answering())
                            .waiting(self.waits())
                    },
                )
            })
            .map(Interacted::new);
        Box::pin(std::future::ready(Ok(interacting)))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        let route = placed.of::<RouteOptions>()?;
        let teams = match self.registered().ok()? {
            Some(registered) => Teams::bot(
                Bot::new(registered.app_id, secret, registered.hosts, route).worded(
                    self.interactions
                        .as_ref()
                        .map(|interactions| Wording::of(interactions.wording.as_ref()))
                        .unwrap_or_default(),
                ),
            ),
            None => Teams::new(secret),
        };
        Some(Reached::new(teams.waiting(self.waits())))
    }
}

impl Placement for RouteOptions {
    fn written(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    fn place(&self) -> Option<&str> {
        self.conversation.as_deref()
    }
}

/// Microsoft Teams, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamsKind {}

impl DestinationKind for TeamsKind {
    const NAME: &str = KIND;
    const DESCRIPTION: &str = "Microsoft Teams, by Workflows webhook or as a bot.";
    const LABEL: &str = TEAMS;
    const ROUTE: &str = "TeamsRoute";
    const ROUTE_DESCRIPTION: &str = "A route to a Microsoft Teams destination.";

    type Options = DestinationOptions;
    type Route = RouteOptions;

    const FIRST_RUN: &[herald_kinds::Variant] = crate::first_run::VARIANTS;

    fn checked(
        options: DestinationOptions,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(DestinationOptions, Vec<Fault>), Fault> {
        options.bot().map_err(Fault::here)?;
        let faults = options
            .registered()
            .err()
            .map(|(key, message)| Fault::at(key, message))
            .into_iter()
            .chain(
                options
                    .interactions
                    .as_ref()
                    .and_then(|interactions| interactions.check().err())
                    .map(|message| Fault::at("interactions", message)),
            )
            .collect();
        Ok((options, faults))
    }

    fn placed(
        options: &DestinationOptions,
        route: &RouteOptions,
        placing: Placing<'_>,
    ) -> Vec<Fault> {
        let bot = options.webhook.is_none();
        let mut faults: Vec<Fault> = route
            .unwritten()
            .map(|message| Fault::at("conversation", message))
            .into_iter()
            .collect();
        if bot && route.conversation.is_none() {
            faults.push(Fault::here(
                "a Microsoft Teams bot posts to a channel or chat; give conversation = \"<its id>\"",
            ));
        }
        if bot
            && route.subscribe.is_some()
            && let Err(message) = placing.offers_subscribing(options.answering())
        {
            faults.push(Fault::at("subscribe", message));
        }
        if !bot && route.subscribe.is_some() {
            faults.push(Fault::at(
                "subscribe",
                "a Workflows webhook only posts, so no member subscribes through it; drop it",
            ));
        }
        if !bot {
            faults.extend(
                [
                    ("conversation", route.conversation.is_some()),
                    ("threads", route.threads.is_some()),
                ]
                .into_iter()
                .filter(|(_, given)| *given)
                .map(|(key, _)| {
                    Fault::at(
                        key,
                        "a Workflows webhook posts where its workflow names, alone; drop it",
                    )
                }),
            );
        }
        faults
    }
}

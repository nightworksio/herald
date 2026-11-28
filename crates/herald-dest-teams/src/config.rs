use herald_core::{Delivery, Profile, SecretRef, Symbols, WaitOptions};
use herald_kinds::{
    Checking, DestinationConfig, DestinationKind, Fault, Placed, Placement, Placing, Reached,
};
use herald_ports::Secret;

use crate::bot::Bot;
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

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        let route = placed.of::<RouteOptions>()?;
        let teams = match self.registered().ok()? {
            Some(registered) => {
                Teams::bot(Bot::new(registered.app_id, secret, registered.hosts, route))
            }
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
            .collect();
        Ok((options, faults))
    }

    fn placed(options: &DestinationOptions, route: &RouteOptions, _: Placing<'_>) -> Vec<Fault> {
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

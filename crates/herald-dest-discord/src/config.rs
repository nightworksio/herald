use std::any::Any;
use std::future::ready;

use herald_core::{
    Answering, Asked, DestinationName, Dialect, Measure, Profile, Room, Scheme, SecretRef, Symbols,
    Text, WaitOptions,
};
use herald_kinds::{
    Carried, DestinationConfig, DestinationKind, Endpoint, Fault, Interacted, Interacts, Placed,
    Placement, Placing, Post, Reached, Read, Replying, Said, Session, same,
};
use herald_ports::{Boxed, DynSecrets, Request, Response, Secret, SecretError};

use crate::discord::{self, Credential, Discord};
use crate::held::Joined;
use crate::interact::{self, Interacting, MESSAGE, Received, choices, pong, took};
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

    fn interacts<'a>(
        &'a self,
        credential: &'a Secret,
        _secrets: &'a dyn DynSecrets,
    ) -> Boxed<'a, Result<Option<Interacted>, SecretError>> {
        let bot = self.token.is_some().then(|| credential.clone());
        Box::pin(ready(Ok(self.interactions.as_ref().map(|interactions| {
            Interacted::new(Interacting::new(
                interactions.clone(),
                bot,
                self.api.as_deref(),
            ))
        }))))
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

impl Interacts for Interacting {
    fn api(&self) -> &str {
        Self::api(self)
    }

    fn answering(&self) -> Answering {
        Self::answering(self)
    }

    fn room(&self) -> (Dialect, Room) {
        (
            Dialect::Discord,
            Room {
                measure: Measure::Chars,
                first: MESSAGE,
                rest: MESSAGE,
            },
        )
    }

    fn answered(&self, replying: &Replying, parts: &[Text], public: bool) -> Vec<Request> {
        let Some(replying) = replying.of::<interact::Replying>() else {
            return Vec::new();
        };
        let bodies: Vec<String> = parts.iter().map(|part| part.body.clone()).collect();
        Self::answered(self, replying, &bodies, public)
    }

    fn took(&self, response: &Response) -> Result<(), String> {
        took(response)
    }

    fn deferred(&self, replying: &Replying, asked: &Asked) -> Option<Request> {
        let replying = replying.of::<interact::Replying>()?;
        let public = Self::answering(self)
            .replies
            .public(asked.command, asked.share);
        Some(self.defer(replying, public))
    }

    fn posted(&self, endpoint: Endpoint) -> Option<Result<(Scheme, &str), &'static str>> {
        (endpoint == Endpoint::Posted).then(|| {
            self.key()
                .map(|key| (Scheme::Discord, key))
                .ok_or("interactions for such a destination")
        })
    }

    fn read(&self, destination: &DestinationName, post: Post<'_>) -> Read {
        match self.received(destination, post.body) {
            Ok(Received::Ping) => Read::Said(Said::ok(pong())),
            Ok(Received::Completing { place, typed, .. }) => Read::Completing { place, typed },
            Ok(Received::Asked { asked, replying }) => {
                let public = Self::answering(self)
                    .replies
                    .public(asked.command, asked.share);
                Read::Asked {
                    asked: vec![(asked, Replying::new(replying))],
                    answer: Said::ok(interact::deferred(public)),
                }
            }
            Ok(Received::Ignored) => {
                Read::Refused("the interaction is of a kind herald does not answer".to_owned())
            }
            Err(reason) => Read::Unread(reason.to_string()),
        }
    }

    fn choices(&self, projects: &[String]) -> String {
        choices(projects)
    }

    fn session(&self) -> Option<Box<dyn Session + '_>> {
        Joined::of(self).map(|session| Box::new(session) as Box<dyn Session + '_>)
    }

    fn complete(&self, replying: &Replying, projects: &[String]) -> Option<Request> {
        replying
            .of::<interact::Replying>()
            .map(|replying| Self::complete(self, replying, projects))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

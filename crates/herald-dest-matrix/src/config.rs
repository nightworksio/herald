use herald_core::{
    Answering, DestinationName, Dialect, Profile, Room, Scheme, SecretRef, Symbols, Text,
    WaitOptions,
};
use herald_kinds::{
    Carried, DestinationConfig, DestinationKind, Endpoint, Fault, Interacted, Interacts, Placed,
    Placement, Placing, Post, Reached, Read, Replying, Said, Session, optional,
};
use herald_ports::{Boxed, DynSecrets, Request, Response, Secret, SecretError};
use jiff::SignedDuration;

use crate::held::Syncing;
use crate::interact::{self, Interacting, took};
use crate::interactions::{Interactions, Via};
use crate::matrix::{self, Matrix};
use crate::options::{DestinationOptions, RouteOptions};

/// What a homeserver is answered for a call that went through.
const EMPTY: &str = "{}";

/// What a homeserver is answered for a user or room herald does not hold.
fn unknown() -> Said {
    Said {
        status: 404,
        body: r#"{"errcode":"M_NOT_FOUND"}"#.to_owned(),
    }
}

/// The kind of a Matrix destination, as the config spells it.
pub const KIND: &str = "matrix";

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
        Some(&self.token)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        let interactions = self.interactions.as_ref();
        [Some(&self.token)]
            .into_iter()
            .chain([
                interactions.and_then(|interactions| interactions.homeserver_token.as_ref()),
                interactions.and_then(|interactions| interactions.appservice_token.as_ref()),
            ])
            .flatten()
            .collect()
    }

    fn answering(&self) -> Option<Answering> {
        self.interactions.as_ref().map(Interactions::answering)
    }

    fn carried(&self) -> Option<Carried> {
        self.interactions.as_ref().map(|interactions| Carried {
            posted: interactions.via == Via::Appservice,
            what: "transactions the homeserver pushes",
            session: "a sync with via = \"sync\"",
        })
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<RouteOptions>()?;
        Some((matrix::profile(), None))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        let placed = placed.of::<RouteOptions>()?;
        Some(Reached::new(
            Matrix::new(&self.homeserver, secret, placed.clone())
                .reading_back(self.read_back.unwrap_or_default())
                .waiting(self.waits()),
        ))
    }

    fn interacts<'a>(
        &'a self,
        credential: &'a Secret,
        secrets: &'a dyn DynSecrets,
    ) -> Boxed<'a, Result<Option<Interacted>, SecretError>> {
        Box::pin(async move {
            let Some(interactions) = &self.interactions else {
                return Ok(None);
            };
            let acting = match interactions.via {
                Via::Appservice => {
                    optional(secrets, interactions.appservice_token.as_ref()).await?
                }
                Via::Sync => None,
            };
            let presented = optional(secrets, interactions.homeserver_token.as_ref()).await?;
            Ok(Some(Interacted::new(
                Interacting::new(
                    &self.homeserver,
                    interactions.clone(),
                    acting.unwrap_or_else(|| credential.clone()),
                )
                .presented(presented),
            )))
        })
    }
}

impl Placement for RouteOptions {
    fn place(&self) -> Option<&str> {
        Some(&self.room)
    }
}

/// Matrix, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixKind {}

impl DestinationKind for MatrixKind {
    const NAME: &str = KIND;
    const DESCRIPTION: &str = "A bot account on a Matrix homeserver.";
    const LABEL: &str = "Matrix";
    const ROUTE: &str = "MatrixRoute";
    const ROUTE_DESCRIPTION: &str = "A route to a Matrix destination.";

    type Options = DestinationOptions;
    type Route = RouteOptions;

    fn checked(
        options: DestinationOptions,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(DestinationOptions, Vec<Fault>), Fault> {
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
        _options: &DestinationOptions,
        route: &RouteOptions,
        _placing: Placing<'_>,
    ) -> Vec<Fault> {
        route.check().err().map(Fault::here).into_iter().collect()
    }

    fn outlasts(options: &DestinationOptions, total: SignedDuration) -> Option<Fault> {
        let wait = options.interactions.as_ref()?.sync_wait();
        (wait >= total).then(|| {
            Fault::at(
                "interactions",
                format!(
                    "a sync that waits {wait:#} outlasts the {total:#} [http] total allows any answer; make sync_wait shorter, or total longer"
                ),
            )
        })
    }
}

impl Interacts for Interacting {
    fn api(&self) -> &str {
        Self::api(self)
    }

    fn answering(&self) -> Answering {
        Self::answering(self)
    }

    fn room(&self) -> (Dialect, Room) {
        let profile = matrix::profile();
        (
            Dialect::Matrix,
            Room {
                measure: profile.measure,
                first: profile.limits.followup,
                rest: profile.limits.followup,
            },
        )
    }

    fn answered(&self, replying: &Replying, parts: &[Text], public: bool) -> Vec<Request> {
        replying
            .of::<interact::Replying>()
            .map(|replying| Self::answered(self, replying, parts, public))
            .unwrap_or_default()
    }

    fn took(&self, response: &Response) -> Result<(), String> {
        took(response)
    }

    fn posted(&self, endpoint: Endpoint) -> Option<Result<(Scheme, &str), &'static str>> {
        (endpoint == Endpoint::Pushed).then(|| {
            self.homeserver_token()
                .map(|token| (Scheme::Matrix, token.expose()))
                .ok_or("application service for such a destination")
        })
    }

    fn read(&self, destination: &DestinationName, post: Post<'_>) -> Read {
        match (post.method, post.path) {
            ("PUT", ["_matrix", "app", "v1", "transactions", transaction]) => Read::Delivered {
                delivery: format!("{}/{transaction}", destination.as_str()),
                replayed: Said::ok(EMPTY),
                then: Box::new(match self.transaction(destination, post.body) {
                    Ok(asked) if asked.is_empty() => Read::Said(Said::ok(EMPTY)),
                    Ok(asked) => Read::Asked {
                        asked: asked
                            .into_iter()
                            .map(|(asked, replying)| (asked, Replying::new(replying)))
                            .collect(),
                        answer: Said::ok(EMPTY),
                    },
                    Err(reason) => Read::Unread(reason.to_string()),
                }),
            },
            ("GET", ["_matrix", "app", "v1", "users", user]) => {
                let owned = user
                    .strip_prefix('@')
                    .and_then(|user| user.split_once(':'))
                    .is_some_and(|(local, _)| local == self.sender());
                Read::Said(if owned { Said::ok(EMPTY) } else { unknown() })
            }
            ("GET", ["_matrix", "app", "v1", "rooms", _]) => Read::Said(unknown()),
            ("POST", ["_matrix", "app", "v1", "ping"]) => Read::Said(Said::ok(EMPTY)),
            (
                _,
                [
                    "_matrix",
                    "app",
                    "v1",
                    "transactions" | "users" | "rooms" | "ping",
                    ..,
                ],
            ) => Read::Unmethodical,
            _ => Read::Missing("such path"),
        }
    }

    fn session(&self) -> Option<Box<dyn Session + '_>> {
        Syncing::of(self).map(|session| Box::new(session) as Box<dyn Session + '_>)
    }
}

#[cfg(test)]
mod tests;

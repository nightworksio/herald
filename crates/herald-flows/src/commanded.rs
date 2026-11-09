//! Commands posted to herald's endpoint: Discord interactions and Slack
//! requests at `/interactions/<destination>`, and a Matrix homeserver's
//! application-service calls under `/matrix/<destination>`, each verified
//! and admitted before it is read, and answered at once with the command's
//! work left for after (ADR-0007, ADR-0016).

use herald_core::{Asked, DestinationName, Scheme};
use herald_dest_discord as discord;
use herald_dest_matrix as matrix;
use herald_dest_slack as slack;
use herald_kinds::Replying;
use herald_ports::{Clock, Keyring, Store, Verifier};

use crate::inbound::{Checking, Rejected, Verified, admitted, verified};
use crate::interact::completed;
use crate::serving::{
    Answered, Inbound, Job, Serving, missing, refusal, rejection, replied, unmethodical, unread,
};

/// Where Discord and Slack post commands, before the destination's name.
pub const INTERACTIONS: &str = "/interactions/";
/// Where a homeserver reaches herald as an application service, before the
/// destination's name.
pub const MATRIX: &str = "/matrix/";
/// The media type of Discord's, Slack's and Matrix's answers.
const JSON: &str = "application/json";
/// What a homeserver is answered for a call that went through.
const EMPTY: &str = "{}";

fn job(destination: &DestinationName, asked: Vec<(Asked, Replying)>) -> Job {
    Job::Interact {
        destination: destination.clone(),
        asked,
    }
}

/// A command posted to the destination `name`.
pub(crate) async fn commanded<S: Store, C: Clock, G, V: Verifier, H, P, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    name: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let destination = DestinationName::new(name);
    let interacting = serving
        .connected
        .interacting(&destination)
        .map(|(interacting, _)| interacting);
    if let Some(interacting) = interacting.and_then(|found| found.of::<discord::Interacting>()) {
        return interaction(serving, &destination, interacting, inbound).await;
    }
    if let Some(interacting) = interacting.and_then(|found| found.of::<slack::Interacting>())
        && let Some(signing) = interacting.signing_secret()
    {
        return slash(
            serving,
            &destination,
            interacting,
            signing.expose(),
            inbound,
        )
        .await;
    }
    missing("commands for such a destination")
}

async fn admit<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    scheme: Scheme,
    written: &str,
    inbound: &Inbound<'_>,
) -> Result<Verified, Rejected> {
    let now = serving.ports.clock.now();
    let verified = verified(
        serving.verifier,
        scheme,
        written,
        inbound.headers,
        inbound.body,
        Checking {
            now,
            bounds: &serving.config.server.inbound,
        },
    )
    .await?;
    admitted(
        serving.ports.store,
        serving.admission,
        scheme,
        &verified,
        now,
    )
    .await?;
    Ok(verified)
}

async fn interaction<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    destination: &DestinationName,
    interacting: &discord::Interacting,
    inbound: &Inbound<'_>,
) -> Answered {
    let Some(key) = interacting.key() else {
        return missing("interactions for such a destination");
    };
    let verified = match admit(serving, Scheme::Discord, key, inbound).await {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    match interacting.received(destination, inbound.body) {
        Ok(discord::Received::Ping) => replied(200, JSON, discord::pong()),
        Ok(discord::Received::Completing { place, typed, .. }) => {
            let projects = completed(
                serving.config,
                serving.ports.store,
                destination,
                &place,
                &typed,
            )
            .await;
            replied(200, JSON, discord::choices(&projects))
        }
        Ok(discord::Received::Asked { asked, replying }) => {
            let public = interacting
                .answering()
                .replies
                .public(asked.command, asked.share);
            Answered {
                job: Some(job(destination, vec![(asked, Replying::new(replying))])),
                ..replied(200, JSON, discord::deferred(public))
            }
        }
        Ok(discord::Received::Ignored) => {
            refusal(400, "the interaction is of a kind herald does not answer")
        }
        Err(reason) => unread(serving.ports.store, Scheme::Discord, &verified, reason).await,
    }
}

async fn slash<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    destination: &DestinationName,
    interacting: &slack::Interacting,
    signing: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let verified = match admit(serving, Scheme::Slack, signing, inbound).await {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    match interacting.received(destination, inbound.body) {
        Ok(slack::Received::Asked { asked, replying }) => Answered {
            job: Some(job(destination, vec![(*asked, Replying::new(replying))])),
            ..replied(200, JSON, "")
        },
        Ok(slack::Received::Usage) => replied(200, JSON, interacting.usage()),
        Ok(slack::Received::Ignored) => replied(200, JSON, ""),
        Err(reason) => unread(serving.ports.store, Scheme::Slack, &verified, reason).await,
    }
}

/// A homeserver's call to the application service of the destination
/// `name`, at `rest` after its name: each checked against the token the
/// homeserver presents.
pub(crate) async fn appserviced<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    name: &str,
    rest: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let destination = DestinationName::new(name);
    let Some((interacting, token)) = serving
        .connected
        .interacting(&destination)
        .and_then(|(found, _)| found.of::<matrix::Interacting>())
        .and_then(|interacting| Some((interacting, interacting.homeserver_token()?)))
    else {
        return missing("application service for such a destination");
    };
    let now = serving.ports.clock.now();
    let checked = verified(
        serving.verifier,
        Scheme::Matrix,
        token.expose(),
        inbound.headers,
        inbound.body,
        Checking {
            now,
            bounds: &serving.config.server.inbound,
        },
    )
    .await;
    if let Err(rejected) = checked {
        return rejection(&rejected);
    }
    let parts: Vec<&str> = rest.split('/').collect();
    match (inbound.method, parts.as_slice()) {
        ("PUT", ["_matrix", "app", "v1", "transactions", transaction]) => {
            let delivery = Verified {
                delivery: Some(format!("{name}/{transaction}")),
                signer: None,
            };
            match admitted(
                serving.ports.store,
                serving.admission,
                Scheme::Matrix,
                &delivery,
                now,
            )
            .await
            {
                Ok(()) => {}
                Err(Rejected::Replayed) => return replied(200, JSON, EMPTY),
                Err(rejected) => return rejection(&rejected),
            }
            match interacting.transaction(&destination, inbound.body) {
                Ok(asked) if asked.is_empty() => replied(200, JSON, EMPTY),
                Ok(asked) => Answered {
                    job: Some(job(
                        &destination,
                        asked
                            .into_iter()
                            .map(|(asked, replying)| (asked, Replying::new(replying)))
                            .collect(),
                    )),
                    ..replied(200, JSON, EMPTY)
                },
                Err(reason) => unread(serving.ports.store, Scheme::Matrix, &delivery, reason).await,
            }
        }
        ("GET", ["_matrix", "app", "v1", "users", user]) => {
            let owned = user
                .strip_prefix('@')
                .and_then(|user| user.split_once(':'))
                .is_some_and(|(local, _)| local == interacting.sender());
            if owned {
                replied(200, JSON, EMPTY)
            } else {
                replied(404, JSON, r#"{"errcode":"M_NOT_FOUND"}"#)
            }
        }
        ("GET", ["_matrix", "app", "v1", "rooms", _]) => {
            replied(404, JSON, r#"{"errcode":"M_NOT_FOUND"}"#)
        }
        ("POST", ["_matrix", "app", "v1", "ping"]) => replied(200, JSON, EMPTY),
        (
            _,
            [
                "_matrix",
                "app",
                "v1",
                "transactions" | "users" | "rooms" | "ping",
                ..,
            ],
        ) => unmethodical(),
        _ => missing("such path"),
    }
}

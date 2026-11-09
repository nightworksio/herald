//! Commands posted to herald's endpoint: requests carrying commands at
//! `/interactions/<destination>`, and transactions a server pushes under
//! `/matrix/<destination>`, each verified with what its destination's kind
//! names and admitted before it is read, and answered at once with the
//! command's work left for after (ADR-0007, ADR-0016).

use herald_core::{Asked, DestinationName, Scheme};
use herald_kinds::{Endpoint, Interacted, Post, Read, Replying, Said};
use herald_ports::{Clock, Keyring, Store, Verifier};

use crate::inbound::{Checking, Rejected, Verified, admitted, verified};
use crate::interact::completed;
use crate::serving::{
    Answered, Inbound, Job, Serving, missing, refusal, rejection, replied, unmethodical, unread,
};

/// Where requests carrying commands are posted, before the destination's
/// name.
pub const INTERACTIONS: &str = "/interactions/";
/// Where a server pushes transactions, before the destination's name.
pub const MATRIX: &str = "/matrix/";
/// The media type of every answer an endpoint gives.
const JSON: &str = "application/json";

fn job(destination: &DestinationName, asked: Vec<(Asked, Replying)>) -> Job {
    Job::Interact {
        destination: destination.clone(),
        asked,
    }
}

fn said(said: Said) -> Answered {
    replied(said.status, JSON, said.body)
}

/// A request to `endpoint` for the destination `name`, at `rest` after its
/// name: verified with the scheme and secret its kind names, admitted once,
/// and answered as it reads.
pub(crate) async fn commanded<S: Store, C: Clock, G, V: Verifier, H, P, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    endpoint: Endpoint,
    name: &str,
    rest: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let destination = DestinationName::new(name);
    let unset = match endpoint {
        Endpoint::Posted => "commands for such a destination",
        Endpoint::Pushed => "application service for such a destination",
    };
    let Some((interacting, _)) = serving.connected.interacting(&destination) else {
        return missing(unset);
    };
    let (scheme, written) = match interacting.posted(endpoint) {
        None => return missing(unset),
        Some(Err(what)) => return missing(what),
        Some(Ok(posted)) => posted,
    };
    let verified = match admit(serving, scheme, written, inbound).await {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    let path: Vec<&str> = if rest.is_empty() {
        Vec::new()
    } else {
        rest.split('/').collect()
    };
    let read = interacting.read(
        &destination,
        Post {
            method: inbound.method,
            path: &path,
            body: inbound.body,
        },
    );
    answered(serving, &destination, interacting, scheme, &verified, read).await
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

async fn answered<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    destination: &DestinationName,
    interacting: &Interacted,
    scheme: Scheme,
    verified: &Verified,
    read: Read,
) -> Answered {
    match read {
        Read::Asked { asked, answer } => Answered {
            job: Some(job(destination, asked)),
            ..said(answer)
        },
        Read::Completing { place, typed } => {
            let projects = completed(
                serving.config,
                serving.ports.store,
                destination,
                &place,
                &typed,
            )
            .await;
            replied(200, JSON, interacting.choices(&projects))
        }
        Read::Said(answer) => said(answer),
        Read::Delivered {
            delivery,
            replayed,
            then,
        } => {
            let delivery = Verified {
                delivery: Some(delivery),
                signer: None,
            };
            match admitted(
                serving.ports.store,
                serving.admission,
                scheme,
                &delivery,
                serving.ports.clock.now(),
            )
            .await
            {
                Ok(()) => {}
                Err(Rejected::Replayed) => return said(replayed),
                Err(rejected) => return rejection(&rejected),
            }
            Box::pin(answered(
                serving,
                destination,
                interacting,
                scheme,
                &delivery,
                *then,
            ))
            .await
        }
        Read::Refused(reason) => refusal(400, reason),
        Read::Unread(reason) => unread(serving.ports.store, scheme, verified, reason).await,
        Read::Unmethodical => unmethodical(),
        Read::Missing(what) => missing(what),
    }
}

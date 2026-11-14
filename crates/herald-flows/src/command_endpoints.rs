//! Commands posted to herald's endpoint: requests carrying commands at
//! `/interactions/<destination>`, and transactions a server pushes under
//! `/matrix/<destination>`, each verified with what its destination's kind
//! names and admitted before it is read, and answered at once with the
//! command's work left for after (ADR-0007, ADR-0016).

use herald_core::{Asked, DestinationName, Place, Scheme};
use herald_core::{media, status};
use herald_kinds::{Endpoint, Interacted, Post, Read, Replying, Said};
use herald_ports::{Clock, Keyring, Store, Verifier};

use crate::command_answers::{completed, unlisted};
use crate::serving::{
    Answered, Inbound, Job, Serving, missing, refusal, rejection, replied, unmethodical, unread,
};
use crate::verification::{Checking, Rejected, Verified, admitted, verified};

/// Where requests carrying commands are posted, before the destination's
/// name.
pub(crate) const INTERACTIONS: &str = "/interactions/";
/// Where a server pushes transactions, before the destination's name.
pub(crate) const MATRIX: &str = "/matrix/";

fn job(destination: &DestinationName, asked: Vec<(Asked, Replying)>) -> Job {
    Job::Interact {
        destination: destination.clone(),
        asked,
    }
}

/// What a destination's kind says, answered as JSON.
fn json_reply(said: Said) -> Answered {
    replied(said.status, media::JSON, said.body)
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
            ..json_reply(answer)
        },
        Read::Completing { place, typed } => {
            completions(serving, destination, interacting, &place, &typed).await
        }
        Read::Said(answer) => json_reply(answer),
        Read::Delivered {
            delivery,
            replayed,
            then,
        } => {
            Box::pin(redelivered(
                serving,
                destination,
                interacting,
                scheme,
                delivery,
                replayed,
                *then,
            ))
            .await
        }
        Read::Refused(reason) => refusal(status::BAD_REQUEST, reason),
        Read::Unread(reason) => unread(serving.ports.store, scheme, verified, reason).await,
        Read::Unmethodical => unmethodical(),
        Read::Missing(what) => missing(what),
    }
}

/// The projects announced on `destination` in `place` whose names hold
/// `typed`, offered as `interacting` offers choices.
async fn completions<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    destination: &DestinationName,
    interacting: &Interacted,
    place: &Place,
    typed: &str,
) -> Answered {
    match completed(
        serving.config,
        serving.ports.store,
        destination,
        place,
        typed,
    )
    .await
    {
        Ok(projects) => replied(status::OK, media::JSON, interacting.choices(&projects)),
        Err(error) => refusal(status::UNAVAILABLE, unlisted(&error)),
    }
}

/// What `then` says, once the delivery a request carries inside it is
/// admitted once under `scheme`; a delivery admitted before is answered
/// `replayed`.
async fn redelivered<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    destination: &DestinationName,
    interacting: &Interacted,
    scheme: Scheme,
    delivery: String,
    replayed: Said,
    then: Read,
) -> Answered {
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
        Err(Rejected::Replayed) => return json_reply(replayed),
        Err(rejected) => return rejection(&rejected),
    }
    answered(serving, destination, interacting, scheme, &delivery, then).await
}

//! herald's ActivityPub actors as the server answers for them: their
//! documents, Notes, outboxes, followers, inboxes and WebFinger.

use herald_core::status;
use herald_core::{ACTIVITY_JSON, Addressed, DestinationName, Escaping, Key};
use herald_kinds::Federates;
use herald_ports::{
    Clock, CryptoError, Http, Keyring, Notes, Outboxed, Signer, Store, StoreError, Subscriptions,
    Verifier,
};

use super::jobs::Job;
use super::replies::{Answered, gone, missing, refusal, rejection, replied, text_reply};
use super::{Inbound, Serving};
use crate::acting::{Found, Standing, Wanted, found};
use crate::prepare::Connected;
use crate::verification::{Checking, activity_verified, admitted};

/// What WebFinger answers as.
const JRD: &str = "application/jrd+json";

/// The actor herald serves as `name` on `host`, as the store keeps its
/// project's members and followers now.
pub(super) async fn actor<'a, S: Store, C: Clock, G, V, H, P, K>(
    serving: &Serving<'a, S, C, G, V, H, P, K>,
    name: &str,
    host: &str,
) -> Result<Found<'a, H>, StoreError> {
    found(
        serving.connected,
        serving.ports.store,
        Wanted::Named { name, host },
        serving.ports.clock.now(),
    )
    .await
}

/// The domain from herald's config that is `host`, when herald serves an
/// actor on it.
pub(super) fn domain<'a, H>(connected: &'a Connected<H>, host: &str) -> Option<&'a str> {
    connected.routes.iter().find_map(|(route, _)| {
        let actor = route.reached.federates()?;
        (actor.domain() == host).then_some(actor.domain())
    })
}

/// What WebFinger answers for the resource `query` asks for: an actor herald
/// serves, as `acct:<actor>@<domain>` or as its id.
pub(super) async fn fingered<S: Store, C: Clock, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    query: Option<&str>,
) -> Answered {
    let resource = query
        .unwrap_or_default()
        .split('&')
        .find_map(|pair| pair.strip_prefix("resource="))
        .and_then(|resource| Escaping::UNRESERVED.unescape(resource))
        .unwrap_or_default();
    let wanted = resource
        .strip_prefix("acct:")
        .and_then(|account| account.rsplit_once('@'))
        .map_or(Wanted::Id(&resource), |(name, host)| Wanted::Named {
            name,
            host,
        });
    let now = serving.ports.clock.now();
    match found(serving.connected, serving.ports.store, wanted, now).await {
        Ok(Found::Acting(acting)) if acting.standing == Standing::Gone => gone(),
        Ok(Found::Acting(acting)) => replied(status::OK, JRD, acting.actor().finger()),
        Ok(Found::Ambiguous(why)) => text_reply(status::NOT_FOUND, &why),
        Ok(Found::Missing) => missing("such actor"),
        Err(error) => refusal(status::UNAVAILABLE, error.to_string()),
    }
}

/// The actor document of `served`.
pub(super) async fn documented<S, C, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    served: &dyn Federates,
) -> Answered {
    document_reply(served.document(serving.ports.signer).await)
}

/// The answer to an actor document that was or could not be built.
fn document_reply(document: Result<String, CryptoError>) -> Answered {
    match document {
        Ok(document) => replied(status::OK, ACTIVITY_JSON, document),
        Err(error) => refusal(
            status::SERVER_ERROR,
            format!("the actor's key cannot be read: {error}"),
        ),
    }
}

/// The Note `served` published as the announcement `key`.
pub(super) async fn noted<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    served: &dyn Federates,
    key: &str,
) -> Answered {
    let Ok(parsed) = key.parse::<Key>() else {
        return missing("such note");
    };
    match serving.ports.store.notes().note(&parsed).await {
        Ok(Some(note)) if served.owns(&note, key) => replied(status::OK, ACTIVITY_JSON, note),
        Ok(_) => missing("such note"),
        Err(error) => refusal(status::UNAVAILABLE, error.to_string()),
    }
}

/// The outbox of `served`, or the page of it `query` asks for.
pub(super) async fn outboxed<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    served: &dyn Federates,
    query: Option<&str>,
) -> Answered {
    let page = query.and_then(|query| query.split('&').find_map(|pair| pair.strip_prefix("page=")));
    let after = match page {
        None => None,
        Some(name) if name == served.first_page() => None,
        Some(name) if Outboxed::is_name(name) => Some(name),
        Some(_) => return missing("such page"),
    };
    let notes = serving.ports.store.notes();
    let most = if page.is_some() {
        served.outbox_most()
    } else {
        0
    };
    match notes.outbox(served.actor(), after, most).await {
        Ok(listed) => replied(
            status::OK,
            ACTIVITY_JSON,
            match page {
                None => served.outbox(listed.total),
                Some(page) => served.outbox_page(page, &listed.notes, listed.next.as_deref()),
            },
        ),
        Err(error) => refusal(status::UNAVAILABLE, error.to_string()),
    }
}

/// How many follow `served`, the actor of `destination`, and never who.
pub(super) async fn counted<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    served: &dyn Federates,
    destination: &DestinationName,
) -> Answered {
    match serving
        .ports
        .store
        .subscriptions()
        .subscribers(destination, served.actor())
        .await
    {
        Ok(following) => {
            let admitted = following
                .iter()
                .filter(|kept| kept.pending.is_none() && served.admits(&kept.subscriber))
                .count();
            replied(status::OK, ACTIVITY_JSON, served.followers(admitted))
        }
        Err(error) => refusal(status::UNAVAILABLE, error.to_string()),
    }
}

/// An activity delivered to an inbox herald serves, verified and admitted,
/// and left to take.
pub(super) async fn inboxed<S: Store, C: Clock, G, V: Verifier, H, P: Http, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    audience: &str,
) -> Answered {
    let target = match inbound.query {
        Some(query) => format!("post {}?{query}", inbound.path),
        None => format!("post {}", inbound.path),
    };
    let addressed = Addressed {
        target: &target,
        audience,
    };
    let now = serving.ports.clock.now();
    let checked = activity_verified(
        serving.verifier,
        serving.public,
        serving.keys,
        addressed,
        inbound.headers,
        inbound.body,
        Checking {
            now,
            bounds: &serving.config.server.inbound,
        },
    )
    .await;
    let verified = match checked {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    if let Err(rejected) = admitted(
        serving.ports.store,
        serving.admission,
        herald_dest_fediverse::ACTOR_SCHEME.name,
        &verified,
        now,
    )
    .await
    {
        return rejection(&rejected);
    }
    Answered {
        reply: text_reply(status::ACCEPTED, "herald takes the activity").reply,
        refused: None,
        job: Some(Job::Follow {
            verified,
            body: inbound.body.to_vec(),
        }),
    }
}

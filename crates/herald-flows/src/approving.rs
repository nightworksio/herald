use herald_core::{DestinationName, Sent, Subscription};
use herald_dest_fediverse::ActivityPub;
use herald_ports::{Http, Signer, Store, Subscriptions};
use jiff::Timestamp;

use crate::following::{FollowError, Followed, following, kept};
use crate::prepare::Connected;
use crate::reached::Reached;

/// What the operator decides of a follow waiting for approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Keep the follower, and answer its follow with an `Accept`.
    Approve,
    /// Forget the follower, and answer its follow with a `Reject`.
    Reject,
}

/// The actor named `actor` that `destination` serves, with the `Http` it
/// reaches inboxes through.
fn served<'a, H>(
    connected: &'a Connected<H>,
    destination: &DestinationName,
    actor: &str,
) -> Result<(&'a ActivityPub, &'a H), FollowError> {
    connected
        .routes
        .iter()
        .find_map(|(route, http)| match &route.reached {
            Reached::ActivityPub(served)
                if served.actor() == actor && &route.config.route.destination == destination =>
            {
                Some((served, http))
            }
            _ => None,
        })
        .ok_or_else(|| {
            FollowError::Unknown(format!(
                "the destination \"{}\" serves no actor \"{actor}\"",
                destination.as_str()
            ))
        })
}

/// Every follower the actor `actor` of `destination` keeps, approved or
/// waiting, for the operator to read.
///
/// # Errors
///
/// When herald serves no such actor, or the store cannot be read.
pub async fn followers<S: Store, H>(
    connected: &Connected<H>,
    store: &S,
    destination: &DestinationName,
    actor: &str,
) -> Result<Vec<Subscription>, FollowError> {
    let (served, _) = served(connected, destination, actor)?;
    kept(store, served, destination).await
}

/// Approves or rejects, as `decision` says, the follow of the actor `actor`
/// of `destination` by `follower` that waits for the operator, and answers
/// it signed at `now` through `signer`.
///
/// # Errors
///
/// When herald serves no such actor, no follow of it by `follower` waits,
/// an approval would pass the actor's follower caps, or the store cannot
/// keep or forget the follower.
#[allow(
    clippy::too_many_arguments,
    reason = "a decision names its actor, its follower and the ports it answers through"
)]
pub async fn decide<S: Store, G: Signer, H: Http>(
    connected: &Connected<H>,
    store: &S,
    signer: &G,
    now: Timestamp,
    destination: &DestinationName,
    actor: &str,
    follower: &str,
    decision: Decision,
) -> Result<Followed, FollowError> {
    let (served, http) = served(connected, destination, actor)?;
    let kept = kept(store, served, destination).await?;
    let Some((waiting, id)) = kept.iter().find_map(|kept| {
        (kept.subscriber == follower)
            .then_some(kept.pending.as_deref())
            .flatten()
            .map(|id| (kept, id))
    }) else {
        return Err(FollowError::Unknown(format!(
            "no follow of \"{actor}\" by \"{follower}\" waits for approval"
        )));
    };
    let follow = served.follow_of(id, follower);
    let inbox = waiting.inbox.clone().unwrap_or_default();
    let subscriptions = store.subscriptions();
    let stored = |error: herald_ports::StoreError| FollowError::Store(error.to_string());
    let answer = match decision {
        Decision::Approve => {
            if !served.policy().room(follower, &following(served, &kept)) {
                return Err(FollowError::Full);
            }
            let approved = Subscription {
                pending: None,
                ..waiting.clone()
            };
            subscriptions.subscribe(&approved).await.map_err(stored)?;
            served.accept(&follow)
        }
        Decision::Reject => {
            subscriptions.unsubscribe(waiting).await.map_err(stored)?;
            served.reject(&follow)
        }
    };
    let answered = served
        .post(http, signer, now, &inbox, &answer)
        .await
        .unwrap_or_else(|error| Sent::Refused(error.to_string()));
    let (follower, actor) = (follower.to_owned(), actor.to_owned());
    Ok(match decision {
        Decision::Approve => Followed::Accepted {
            follower,
            actor,
            answered,
        },
        Decision::Reject => Followed::Rejected {
            follower,
            actor,
            answered,
        },
    })
}

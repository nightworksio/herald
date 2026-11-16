//! The operator's decisions on follows waiting for approval, and the
//! followers each actor keeps.

use herald_core::{DestinationName, Sent, Subscription};
use herald_ports::{Http, Signer, Store, Subscriptions};
use jiff::Timestamp;

use crate::acting::{Acting, Found, Standing, Wanted, found};
use crate::follows::{FollowError, answered, following, kept};
use crate::prepare::Connected;

/// What the operator decides of a follow waiting for approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Keep the follower, and answer its follow with an `Accept`.
    Approve,
    /// Forget the follower, and answer its follow with a `Reject`.
    Reject,
}

/// The actor named `actor` that `destination` serves, as the store keeps
/// its project's members and followers at `now`.
async fn served<'a, S: Store, H>(
    connected: &'a Connected<H>,
    store: &S,
    (destination, actor): (&DestinationName, &str),
    now: Timestamp,
) -> Result<Acting<'a, H>, FollowError> {
    let wanted = Wanted::Of {
        name: actor,
        destination,
    };
    match found(connected, store, wanted, now).await? {
        Found::Acting(acting) => Ok(acting),
        Found::Ambiguous(why) => Err(FollowError::Unknown(why)),
        Found::Missing => Err(FollowError::Unknown(format!(
            "the destination \"{}\" serves no actor \"{actor}\"",
            destination.as_str()
        ))),
    }
}

/// Every follower the actor `actor` of `destination` keeps, approved or
/// waiting, for the operator to read, the actor's project's members read
/// as they stand at `now`.
///
/// # Errors
///
/// When herald serves no such actor, or the store cannot be read.
pub async fn followers<S: Store, H>(
    connected: &Connected<H>,
    store: &S,
    destination: &DestinationName,
    actor: &str,
    now: Timestamp,
) -> Result<Vec<Subscription>, FollowError> {
    let served = served(connected, store, (destination, actor), now).await?;
    kept(store, served.actor(), destination).await
}

/// Approves or rejects, as `decision` says, the follow of the actor `actor`
/// of `destination` by `follower` that waits for the operator, and answers
/// it signed at `now` through `signer`: how the follower's inbox took the
/// answer.
///
/// # Errors
///
/// When herald serves no such actor, no follow of it by `follower` waits,
/// an approval would pass the actor's follower caps or is of an actor that
/// takes no new follows, or the store cannot keep or forget the follower.
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
) -> Result<Sent, FollowError> {
    let acting = served(connected, store, (destination, actor), now).await?;
    let (served, http) = (acting.actor(), acting.http);
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
    let inbox = waiting.inbox.clone().unwrap_or_default();
    let subscriptions = store.subscriptions();
    let answer = match decision {
        Decision::Approve => {
            if acting.standing != Standing::Live {
                return Err(FollowError::Unknown(format!(
                    "\"{actor}\" takes no new follows, as its project left its owner's selection; reject the follow"
                )));
            }
            if !served.room(follower, &following(served, &kept)) {
                return Err(FollowError::Full);
            }
            let approved = Subscription {
                pending: None,
                ..waiting.clone()
            };
            subscriptions.subscribe(&approved).await?;
            served.answer(id, follower, true)
        }
        Decision::Reject => {
            subscriptions.unsubscribe(waiting).await?;
            served.answer(id, follower, false)
        }
    };
    Ok(answered(
        served.post(http, signer, now, &inbox, &answer).await,
    ))
}

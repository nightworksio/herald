use std::fmt;

use herald_core::{Sent, Subscription};
use herald_dest_fediverse::{ActivityPub, Asked, asked, inboxes};
use herald_ports::{Http, Signer, Store};
use jiff::Timestamp;

use crate::inbound::{Verified, fetched};
use crate::prepare::{Connected, PreparedRoute};
use crate::reached::Reached;

/// What a follow, or the undoing of one, did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Followed {
    /// The follower is kept, and its follow was answered with an `Accept`.
    Accepted {
        /// The follower's actor id.
        follower: String,
        /// The actor it follows.
        actor: String,
        /// What the follower's inbox answered to the `Accept`.
        answered: Sent,
    },
    /// The follower is forgotten.
    Undone {
        /// The follower's actor id.
        follower: String,
        /// The actor it no longer follows.
        actor: String,
    },
    /// Nothing was done, and why.
    Ignored(String),
}

/// Why a follow could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowError {
    /// The store could not keep or forget the follower.
    Store(String),
    /// The follower's own document could not be read.
    Unfetched(String),
}

impl fmt::Display for FollowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(reason) => write!(f, "the follower could not be kept: {reason}"),
            Self::Unfetched(reason) => {
                write!(f, "the follower's document could not be read: {reason}")
            }
        }
    }
}

fn ignored(reason: &str) -> Followed {
    Followed::Ignored(reason.to_owned())
}

/// The route announcing as the actor `id`, and that actor.
fn serving<'a, H>(
    connected: &'a Connected<H>,
    id: &str,
) -> Option<(&'a PreparedRoute, &'a ActivityPub, &'a H)> {
    connected
        .routes
        .iter()
        .find_map(|(route, http)| match &route.reached {
            Reached::ActivityPub(actor) if actor.id() == id => Some((route, actor, http)),
            _ => None,
        })
}

/// Takes `body`, an activity [`crate::activity_verified`] let through as
/// `verified`: a `Follow` of one of herald's actors keeps its follower,
/// delivered to at the inbox its own document names, and is answered with
/// an `Accept` signed at `now` through `signer`; an `Undo` of such a follow
/// forgets the follower (ADR-0004, decision 7).
///
/// # Errors
///
/// When the store cannot keep or forget the follower, or the follower's
/// document cannot be read.
pub async fn followed<S: Store, G: Signer, H: Http>(
    connected: &Connected<H>,
    store: &S,
    signer: &G,
    now: Timestamp,
    verified: &Verified,
    body: &[u8],
) -> Result<Followed, FollowError> {
    let Some(follower) = verified.signer.as_deref() else {
        return Ok(ignored("it names no signer"));
    };
    match asked(body, follower) {
        Asked::Follow { target, follow } => {
            let Some((route, actor, http)) = serving(connected, &target) else {
                return Ok(ignored("it follows no actor herald serves"));
            };
            let document = fetched(http, follower)
                .await
                .map_err(FollowError::Unfetched)?;
            let Some(inboxes) = inboxes(&document, follower) else {
                return Ok(ignored(
                    "the follower's document names no inbox on its own origin",
                ));
            };
            let subscription = Subscription {
                destination: route.config.route.destination.clone(),
                project: actor.actor().to_owned(),
                subscriber: follower.to_owned(),
                inbox: Some(inboxes.delivered().to_owned()),
            };
            store
                .subscribe(&subscription)
                .await
                .map_err(|error| FollowError::Store(error.to_string()))?;
            let answered = actor
                .post(http, signer, now, &inboxes.personal, &actor.accept(&follow))
                .await
                .unwrap_or_else(|error| Sent::Refused(error.to_string()));
            Ok(Followed::Accepted {
                follower: follower.to_owned(),
                actor: actor.actor().to_owned(),
                answered,
            })
        }
        Asked::Undo { follower, target } => {
            let Some((route, actor, _)) = serving(connected, &target) else {
                return Ok(ignored("it undoes a follow of no actor herald serves"));
            };
            let destination = &route.config.route.destination;
            let kept = store
                .subscribers(destination, actor.actor())
                .await
                .map_err(|error| FollowError::Store(error.to_string()))?;
            for subscription in kept.iter().filter(|kept| kept.subscriber == follower) {
                store
                    .unsubscribe(subscription)
                    .await
                    .map_err(|error| FollowError::Store(error.to_string()))?;
            }
            Ok(Followed::Undone {
                follower,
                actor: actor.actor().to_owned(),
            })
        }
        Asked::Nothing(why) => Ok(Followed::Ignored(why)),
    }
}

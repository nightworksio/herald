use std::fmt;

use herald_core::{DestinationName, Sent, Subscription};
use herald_kinds::{Activity, Federates};
use herald_ports::{Http, HttpError, Signer, Store, StoreError, Subscriptions};

use crate::inbound::{Checking, Verified, fetched};
use crate::prepare::{Connected, PreparedRoute};

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
    /// The follow waits for the operator's approval: the follower is kept,
    /// and not delivered to until it is approved.
    Waiting {
        /// The follower's actor id.
        follower: String,
        /// The actor it asks to follow.
        actor: String,
    },
    /// The follower is turned away, as the actor's follower caps are full:
    /// it is not kept, and its follow was answered with a `Reject`.
    Rejected {
        /// The follower's actor id.
        follower: String,
        /// The actor it asked to follow.
        actor: String,
        /// What the follower's inbox answered to the `Reject`.
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
    /// The operator asked about an actor herald does not serve, or a follow
    /// that is not waiting.
    Unknown(String),
    /// The operator approved a follow past the actor's follower caps.
    Full,
}

impl fmt::Display for FollowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(reason) => write!(f, "the follower could not be kept: {reason}"),
            Self::Unfetched(reason) => {
                write!(f, "the follower's document could not be read: {reason}")
            }
            Self::Unknown(what) => write!(f, "{what}"),
            Self::Full => f.write_str(
                "the actor's followers are full; raise followers.most or followers.per_domain, or reject the follow",
            ),
        }
    }
}

impl From<StoreError> for FollowError {
    fn from(error: StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

fn ignored(reason: &str) -> Followed {
    Followed::Ignored(reason.to_owned())
}

/// What a follower's inbox did with an answer `posted` to it: refused,
/// where it could not be reached.
pub(crate) fn answered(posted: Result<Sent, HttpError>) -> Sent {
    posted.unwrap_or_else(|error| Sent::Refused(error.to_string()))
}

/// The route announcing as the actor `id`, and that actor.
fn serving<'a, H>(
    connected: &'a Connected<H>,
    id: &str,
) -> Option<(&'a PreparedRoute, &'a dyn Federates, &'a H)> {
    connected.routes.iter().find_map(|(route, http)| {
        let actor = route.reached.federates()?;
        (actor.id() == id).then_some((route, actor, http))
    })
}

/// Every follower `actor` keeps on `destination`, waiting or not.
pub(crate) async fn kept<S: Store>(
    store: &S,
    actor: &dyn Federates,
    destination: &DestinationName,
) -> Result<Vec<Subscription>, FollowError> {
    Ok(store
        .subscriptions()
        .subscribers(destination, actor.actor())
        .await?)
}

/// The followers among `kept` that follow `actor` now: approved, and still
/// admitted by its policy.
pub(crate) fn following<'a>(actor: &dyn Federates, kept: &'a [Subscription]) -> Vec<&'a str> {
    kept.iter()
        .filter(|kept| kept.pending.is_none())
        .map(|kept| kept.subscriber.as_str())
        .filter(|kept| actor.admits(kept))
        .collect()
}

/// A follow of one of herald's actors: by whom, the `Follow`'s id, and on
/// which destination.
struct Follow<'a> {
    follower: &'a str,
    id: &'a str,
    destination: &'a DestinationName,
}

/// Takes `follow` of `actor`, reached through `http`: turned away by its
/// lists or caps, kept waiting for the operator, or kept and accepted.
async fn taken<S: Store, G: Signer, H: Http>(
    store: &S,
    signer: &G,
    checking: Checking<'_>,
    actor: &dyn Federates,
    http: &H,
    follow: Follow<'_>,
) -> Result<Followed, FollowError> {
    let Follow {
        follower,
        id,
        destination,
    } = follow;
    if !actor.admits(follower) {
        return Ok(ignored(
            "the actor's block or allow list turns its follower away",
        ));
    }
    let document = fetched(http, follower, checking.bounds)
        .await
        .map_err(FollowError::Unfetched)?;
    let Some(inboxes) = actor.inboxes(&document, follower) else {
        return Ok(ignored(
            "the follower's document names no inbox on its own origin",
        ));
    };
    let kept = kept(store, actor, destination).await?;
    let mine = kept.iter().find(|kept| kept.subscriber == follower);
    if !actor.room(follower, &following(actor, &kept)) {
        let answered = answered(
            actor
                .post(
                    http,
                    signer,
                    checking.now,
                    &inboxes.personal,
                    &actor.answer(id, follower, false),
                )
                .await,
        );
        return Ok(Followed::Rejected {
            follower: follower.to_owned(),
            actor: actor.actor().to_owned(),
            answered,
        });
    }
    let waits = actor.manual() && mine.is_none_or(|mine| mine.pending.is_some());
    let subscription = Subscription {
        destination: destination.clone(),
        project: actor.actor().to_owned(),
        subscriber: follower.to_owned(),
        inbox: Some(inboxes.delivered.clone()),
        pending: waits.then(|| id.to_owned()),
    };
    store.subscriptions().subscribe(&subscription).await?;
    if waits {
        return Ok(Followed::Waiting {
            follower: follower.to_owned(),
            actor: actor.actor().to_owned(),
        });
    }
    let answered = answered(
        actor
            .post(
                http,
                signer,
                checking.now,
                &inboxes.personal,
                &actor.answer(id, follower, true),
            )
            .await,
    );
    Ok(Followed::Accepted {
        follower: follower.to_owned(),
        actor: actor.actor().to_owned(),
        answered,
    })
}

/// Takes `body`, an activity [`crate::activity_verified`] let through as
/// `verified`: a `Follow` of one of herald's actors keeps its follower,
/// delivered to at the inbox its own document names, and is answered with
/// an `Accept` signed at `checking`'s time through `signer`; an `Undo` of such a follow
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
    checking: Checking<'_>,
    verified: &Verified,
    body: &[u8],
) -> Result<Followed, FollowError> {
    let Some(follower) = verified.signer.as_deref() else {
        return Ok(ignored("it names no signer"));
    };
    let activity = connected
        .routes
        .iter()
        .find_map(|(route, _)| route.reached.federates())
        .map_or_else(
            || Activity::Nothing("herald serves no actor".to_owned()),
            |actor| actor.read(body, follower),
        );
    match activity {
        Activity::Follow { target, id } => {
            let Some((route, actor, http)) = serving(connected, &target) else {
                return Ok(ignored("it follows no actor herald serves"));
            };
            let follow = Follow {
                follower,
                id: &id,
                destination: &route.config.route.destination,
            };
            taken(store, signer, checking, actor, http, follow).await
        }
        Activity::Undo { follower, target } => {
            let Some((route, actor, _)) = serving(connected, &target) else {
                return Ok(ignored("it undoes a follow of no actor herald serves"));
            };
            let kept = kept(store, actor, &route.config.route.destination).await?;
            for subscription in kept.iter().filter(|kept| kept.subscriber == follower) {
                store.subscriptions().unsubscribe(subscription).await?;
            }
            Ok(Followed::Undone {
                follower,
                actor: actor.actor().to_owned(),
            })
        }
        Activity::Nothing(why) => Ok(Followed::Ignored(why)),
    }
}

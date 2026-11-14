//! The work a request leaves to be done once it is answered, done, or
//! released undone so its sender's next delivery of it is admitted
//! (ADR-0013).

use herald_core::{Asked, DestinationName, Event, RouteId, Scheme, SourceName};
use herald_kinds::Replying;
use herald_ports::{Clock, Http, Signer, Store};

use super::Serving;
use super::replies::{Answered, busy, unreleased};
use crate::announce::{Announced, Outcome, announce_within};
use crate::command_answers::{Answer, interact, replied};
use crate::follows::{FollowError, Followed, followed};
use crate::verification::{Checking, Verified, released};

/// Work a request leaves to be done once it is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// `event`, delivered by a webhook of `source`, to announce; the
    /// delivery is released where a route stops (ADR-0013).
    Announce {
        /// The source the webhook is of.
        source: SourceName,
        /// The event it delivers.
        event: Event,
        /// The scheme it is signed under.
        scheme: Scheme,
        /// What its signature names.
        verified: Verified,
    },
    /// `event`, posted by the app named `app` to its HTTP API, to announce
    /// to the routes the app may announce to; the delivery is released where
    /// a route stops (ADR-0007, ADR-0013).
    App {
        /// The app's name.
        app: String,
        /// The event it posted.
        event: Event,
        /// What it is delivered as: the event's identity.
        verified: Verified,
    },
    /// An activity an inbox let through, to take.
    Follow {
        /// What its signature names.
        verified: Verified,
        /// The activity.
        body: Vec<u8>,
    },
    /// Commands given on `destination`, each to answer where it says.
    Interact {
        /// The destination they were given on.
        destination: DestinationName,
        /// Each command, with where its answer goes, each already answered
        /// as being worked where its destination waits for that.
        asked: Vec<(Asked, Replying)>,
    },
}

/// What a job did, and why the delivery it came by could not be released
/// where its work stopped and the release failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worked {
    /// What the job did.
    pub did: Did,
    /// Why the delivery could not be released, so its sender's next delivery
    /// of it is refused as a replay until housekeeping forgets it.
    pub unreleased: Option<String>,
}

/// What a job did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Did {
    /// What each route did with the event.
    Announced(Vec<Announced>),
    /// What the activity did.
    Followed(Result<Followed, FollowError>),
    /// What each command was answered, and whether the answer was sent.
    Interacted(Vec<Interacted>),
}

impl Did {
    /// Whether the work stopped before it was done: a route stopped, or the
    /// activity was not taken.
    #[must_use]
    pub fn stopped(&self) -> bool {
        match self {
            Self::Announced(announced) => announced
                .iter()
                .any(|announced| matches!(announced.outcome, Outcome::Stopped { .. })),
            Self::Followed(taken) => taken.is_err(),
            Self::Interacted(_) => false,
        }
    }
}

/// One command, answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interacted {
    /// The command.
    pub asked: Asked,
    /// What herald answered.
    pub answer: Answer,
    /// Whether the answer was sent, or else why not.
    pub sent: Result<(), String>,
}

impl Job {
    /// The namespace and identity of the delivery the job came by, where it
    /// came by one to release.
    #[must_use]
    pub fn delivery(&self) -> Option<(&'static str, &Verified)> {
        match self {
            Self::Announce {
                scheme, verified, ..
            } => Some((scheme.name, verified)),
            Self::App { verified, .. } => Some((herald_sources_app::ADMITTED, verified)),
            Self::Follow { verified, .. } => {
                Some((herald_dest_fediverse::ACTOR_SCHEME.name, verified))
            }
            Self::Interact { .. } => None,
        }
    }
}

/// Releases the delivery `job` came by, as it will not be done, so its
/// sender's next delivery of it is admitted (ADR-0013).
///
/// # Errors
///
/// Where the delivery could not be released, saying so for the log.
pub async fn unworked<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: &Job,
) -> Result<(), String> {
    match job.delivery() {
        Some((namespace, verified)) => released(serving.ports.store, namespace, verified)
            .await
            .map_err(|error| unreleased(&error)),
        None => Ok(()),
    }
}

/// The refusal of a request whose work found no place, asking its sender to
/// wait `[server] busy_retry`, once the delivery `job` came by is released,
/// naming a release that failed for the log.
pub async fn unplaced<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: &Job,
) -> Answered {
    let busy = busy(serving.config.server.busy_retry);
    match unworked(serving, job).await {
        Ok(()) => busy,
        Err(unreleased) => Answered {
            refused: busy.refused.map(|reason| format!("{reason}; {unreleased}")),
            ..busy
        },
    }
}

/// `job` done, under the run name `run`: a delivery whose work stopped or
/// failed is released, so its sender's next delivery of it is admitted
/// (ADR-0013).
pub async fn work<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: Job,
    run: &str,
) -> Worked {
    let store = serving.ports.store;
    let delivery = job
        .delivery()
        .map(|(namespace, verified)| (namespace, verified.clone()));
    let did = match job {
        Job::Announce { source, event, .. } => announced(serving, &source, &event, None, run).await,
        Job::App { app, event, .. } => {
            let within = serving
                .config
                .apps
                .get(&app)
                .map(|app| app.routes.as_slice())
                .unwrap_or_default();
            announced(serving, &SourceName::new(&app), &event, Some(within), run).await
        }
        Job::Follow { verified, body } => {
            let taken = followed(
                serving.connected,
                store,
                serving.ports.signer,
                Checking {
                    now: serving.ports.clock.now(),
                    bounds: &serving.config.server.inbound,
                },
                &verified,
                &body,
            )
            .await;
            Did::Followed(taken)
        }
        Job::Interact { destination, asked } => interacted(serving, &destination, asked).await,
    };
    let stopped = did.stopped();
    let unreleased = match delivery.filter(|_| stopped) {
        Some((namespace, verified)) => released(store, namespace, &verified)
            .await
            .err()
            .map(|error| unreleased(&error)),
        None => None,
    };
    Worked { did, unreleased }
}

/// `event`, from `source`, announced under the run name `run` to every
/// route that takes it, or to the routes `within` names alone where it
/// names any.
async fn announced<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    source: &SourceName,
    event: &Event,
    within: Option<&[RouteId]>,
    run: &str,
) -> Did {
    Did::Announced(
        announce_within(
            serving.config,
            serving.connected,
            serving.ports,
            source,
            event,
            within,
            run,
        )
        .await,
    )
}

/// Each command `asked` on `destination` answered, and each answer sent
/// where it goes.
async fn interacted<S: Store, C, G, V, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    destination: &DestinationName,
    asked: Vec<(Asked, Replying)>,
) -> Did {
    let mut interacted = Vec::new();
    for (asked, replying) in asked {
        let answer = interact(serving.config, serving.connected, serving.ports, &asked).await;
        let sent = replied(serving.connected, destination, &replying, &answer).await;
        interacted.push(Interacted {
            asked,
            answer,
            sent,
        });
    }
    Did::Interacted(interacted)
}

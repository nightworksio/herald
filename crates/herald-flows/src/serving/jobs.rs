//! The work a request leaves to be done once it is answered, done, or
//! released undone so its sender's next delivery of it is admitted
//! (ADR-0013).

use herald_core::{
    Asked, DestinationName, Event, Identity, Listing, Origin, Scheme, SourceName, Visibility,
};
use herald_kinds::Replying;
use herald_ports::{Clock, Cursors, Http, Signer, Store, Verifier};

use super::Serving;
use super::replies::{Answered, busy, unreleased};
use crate::announce::Stop;
use crate::announce::{Announced, Outcome, Within, announce_within};
use crate::command_answers::{Answer, interact, replied};
use crate::follows::{FollowError, Followed, followed};
use crate::poll::{Remembered, Sweep, Swept, polled, remembered, stored};
use crate::prepare::expanded;
use crate::verification::{Checking, Verified, released};
use crate::withdraw::tombstoned;

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
        /// The owner's member it is of, where the source is an owner's:
        /// announced only where the owner's last membership record holds it
        /// (ADR-0030).
        member: Option<Member>,
        /// The scheme it is signed under.
        scheme: Scheme,
        /// What its signature names.
        verified: Verified,
    },
    /// The source `source`, whose webhook delivered a tag, to poll, so the
    /// tag is read in full and announced or held as any tag a poll finds;
    /// the delivery is released where the poll stops (ADR-0032).
    Poll {
        /// The source the webhook is of.
        source: SourceName,
        /// The scheme it is signed under.
        scheme: Scheme,
        /// What its signature names.
        verified: Verified,
        /// Whether the delivery said the owner's repositories changed, so
        /// the owner is listed again first.
        relist: bool,
    },
    /// A GitHub App's delivery, left to each source it reaches as that
    /// source's own job; the delivery is released where any stops
    /// (ADR-0041).
    Fanned {
        /// Each source's job.
        jobs: Vec<Job>,
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
    /// What a peer pushed into the `herald` source `source`, its proof
    /// already kept in the source's cursor: its event to announce, or the
    /// identity its tombstone withdraws with the project the cursor held
    /// it under (ADR-0054).
    Pushed {
        /// The source it was pushed into.
        source: SourceName,
        /// The event to announce.
        event: Option<Event>,
        /// The identity to withdraw, and its project.
        withdrawn: Option<(Identity, Option<String>)>,
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
    /// What polling the source did.
    Polled(Vec<Swept>),
    /// What the activity did.
    Followed(Result<Followed, FollowError>),
    /// What each command was answered, and whether the answer was sent.
    Interacted(Vec<Interacted>),
    /// Nothing, as the owner's last membership record does not hold the
    /// member the event is of, as this says.
    Withheld(Withheld),
    /// What each source an App's delivery reached did with it.
    Fanned(Vec<Did>),
}

/// The owner's member a webhook delivery is of (ADR-0030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The forge's id of the member.
    pub id: String,
    /// Who may see it, as the delivery names it, where it does.
    pub visibility: Option<Visibility>,
}

impl Member {
    /// Who may see the member as far as bounding its routes goes, beside
    /// `kept`, who its membership record says may: public only where the
    /// delivery says so too, and otherwise the stricter of the two, a
    /// delivery that names none counting as private.
    #[must_use]
    pub fn bounded(&self, kept: Visibility) -> Visibility {
        match self.visibility {
            Some(delivered) if delivered == kept => kept,
            Some(delivered) => delivered.max(kept).max(Visibility::Internal),
            None => Visibility::Private,
        }
    }
}

/// Why an owner's member's event was not announced (ADR-0030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Withheld {
    /// The owner's last membership record does not name the member.
    Unlisted,
    /// The member left the owner's selection.
    Left,
    /// The membership record could not be read, as this says.
    Unread(String),
    /// The source's project template names no project for the member, as
    /// this says.
    Unnamed(String),
    /// The member's project template names the project another member or
    /// source announces as, as this says.
    Collides(String),
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
            Self::Polled(swept) => swept
                .iter()
                .any(|swept| matches!(swept.outcome, Sweep::Stopped(_))),
            Self::Interacted(_) => false,
            Self::Withheld(withheld) => matches!(withheld, Withheld::Unread(_)),
            Self::Fanned(each) => each.iter().any(Self::stopped),
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
            }
            | Self::Poll {
                scheme, verified, ..
            }
            | Self::Fanned {
                scheme, verified, ..
            } => Some((scheme.name, verified)),
            Self::App { verified, .. } => Some((herald_sources_app::ADMITTED, verified)),
            Self::Follow { verified, .. } => {
                Some((herald_dest_fediverse::ACTOR_SCHEME.name, verified))
            }
            Self::Interact { .. } | Self::Pushed { .. } => None,
        }
    }
}

/// The owner listing `source` keeps marked stale where `relist` says its
/// repositories changed, so a source discovering by push lists the owner
/// again; nothing to mark where it keeps no listing or one already stale.
///
/// # Errors
///
/// Where the listing could not be read or marked.
async fn stale<S: Store>(store: &S, source: &SourceName, relist: bool) -> Result<(), Stop> {
    let cursors = store.cursors();
    let Some(record) = cursors
        .listing(source)
        .await
        .map_err(stored)?
        .filter(|record| relist && !record.value.stale)
    else {
        return Ok(());
    };
    let listing = Listing {
        stale: true,
        ..record.value
    };
    match remembered(
        cursors
            .put_listing(source, &listing, Some(record.version))
            .await,
        "the owner's listing",
    ) {
        Remembered::Failed(stop) => Err(stop),
        Remembered::Advanced | Remembered::Kept => Ok(()),
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
pub async fn work<S: Store, C: Clock, G: Signer, V: Verifier, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: Job,
    run: &str,
) -> Worked {
    let store = serving.ports.store;
    let delivery = job
        .delivery()
        .map(|(namespace, verified)| (namespace, verified.clone()));
    let did = done(serving, job, run).await;
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

/// What `job` did, under the run name `run`.
async fn done<S: Store, C: Clock, G: Signer, V: Verifier, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: Job,
    run: &str,
) -> Did {
    let store = serving.ports.store;
    match job {
        Job::Announce {
            source,
            event,
            member: Some(member),
            ..
        } => membered(serving, &source, (&event, &member), run).await,
        Job::Announce { source, event, .. } => {
            announced(serving, &source, &event, Within::default(), run).await
        }
        Job::Poll { source, relist, .. } => {
            Did::Polled(match stale(store, &source, relist).await {
                Ok(()) => {
                    polled(
                        serving.config,
                        serving.connected,
                        serving.ports,
                        serving.verifier,
                        (run, Some(&source)),
                    )
                    .await
                }
                Err(stop) => vec![Swept {
                    source,
                    member: None,
                    outcome: Sweep::Stopped(stop),
                }],
            })
        }
        Job::App { app, event, .. } => {
            let within = serving
                .config
                .apps
                .get(&app)
                .map(|app| app.routes.as_slice())
                .unwrap_or_default();
            announced(
                serving,
                &SourceName::new(&app),
                &event,
                Within::routes(within),
                run,
            )
            .await
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
        Job::Pushed {
            source,
            event,
            withdrawn,
        } => Did::Announced(pushed(serving, &source, (event, withdrawn), run).await),
        Job::Fanned { jobs, .. } => {
            let mut each = Vec::new();
            for job in jobs {
                each.push(Box::pin(done(serving, job, run)).await);
            }
            Did::Fanned(each)
        }
    }
}

/// What each route did with what a peer pushed into `source`: its event
/// announced, or the identity its tombstone withdraws withdrawn.
async fn pushed<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    source: &SourceName,
    (event, withdrawn): (Option<Event>, Option<(Identity, Option<String>)>),
    run: &str,
) -> Vec<Announced> {
    let mut announced = Vec::new();
    if let Some(event) = event {
        announced.extend(
            announce_within(
                serving.config,
                serving.connected,
                serving.ports,
                source,
                &event,
                Within::default(),
                run,
            )
            .await,
        );
    }
    if let Some((identity, project)) = withdrawn {
        announced.extend(
            Box::pin(tombstoned(
                serving.config,
                serving.connected,
                serving.ports,
                (source, &identity, project.as_deref()),
                run,
            ))
            .await,
        );
    }
    announced
}

/// `event`, from `source`, announced under the run name `run` to every
/// route that takes it, or to the routes `within` names alone where it
/// names any.
async fn announced<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    source: &SourceName,
    event: &Event,
    within: Within<'_>,
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

/// `event`, from the owner source `source`'s member `id`, announced under
/// the run name `run` where the owner's last membership record holds the
/// member, and only to the `private` routes where it is not public and
/// doctor refuses (ADR-0030).
async fn membered<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    source: &SourceName,
    (event, member): (&Event, &Member),
    run: &str,
) -> Did {
    let held = match serving
        .ports
        .store
        .cursors()
        .member(source, &member.id)
        .await
    {
        Ok(Some(held)) => held.value,
        Ok(None) => return Did::Withheld(Withheld::Unlisted),
        Err(error) => return Did::Withheld(Withheld::Unread(error.to_string())),
    };
    if held.left.is_some() {
        return Did::Withheld(Withheld::Left);
    }
    if let Some(reason) = held.collides {
        return Did::Withheld(Withheld::Collides(reason));
    }
    let expand = expanded(serving.connected, source);
    let named = match expand {
        None => Ok((
            event
                .clone()
                .seen_as(member.bounded(held.repository.visibility)),
            None,
        )),
        Some(expand) => expand.project(&held.repository).and_then(|project| {
            let origin = Origin {
                visibility: member.bounded(held.repository.visibility),
                ..expand.origin(&held.repository)
            };
            event
                .clone()
                .of_project(&project)
                .map(|event| {
                    (
                        event.from_origin(origin),
                        Some(expand.reach(&held.repository)),
                    )
                })
                .map_err(|problem| problem.to_string())
        }),
    };
    let (event, reach) = match named {
        Ok(named) => named,
        Err(reason) => return Did::Withheld(Withheld::Unnamed(reason)),
    };
    let within = Within {
        routes: None,
        reach: reach.as_ref(),
        view: None,
    };
    announced(serving, source, &event, within, run).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_bounded_as_public_only_where_its_delivery_and_its_record_agree() {
        let member = |visibility| Member {
            id: "1".to_owned(),
            visibility,
        };
        let public = Some(Visibility::Public);
        assert_eq!(
            member(public).bounded(Visibility::Public),
            Visibility::Public
        );
        assert_eq!(
            member(Some(Visibility::Private)).bounded(Visibility::Public),
            Visibility::Private
        );
        assert_eq!(
            member(public).bounded(Visibility::Internal),
            Visibility::Internal
        );
        assert_eq!(
            member(public).bounded(Visibility::Private),
            Visibility::Private
        );
        assert_eq!(
            member(Some(Visibility::Internal)).bounded(Visibility::Internal),
            Visibility::Internal
        );
        assert_eq!(
            member(None).bounded(Visibility::Public),
            Visibility::Private
        );
    }
}

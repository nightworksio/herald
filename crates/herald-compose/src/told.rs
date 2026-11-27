//! What a run did, in words: each announcement, poll, chore and job, as
//! every driver says it.

mod merging;

pub use merging::{queue_lines, stepped};

use herald_config::Level;
use herald_core::{Chore, Identity, Printed, Schedule, Step};
use herald_flows::{
    Announced, Chored, Did, FollowError, Followed, Fresh, Housekept, HousekeptSchedule, Interacted,
    Job, Outcome, Remembered, Stop, Sweep, Swept, Withheld, Worked,
};

use crate::Told;

/// How grave a round is whose cursor `remembered` says where it stands:
/// one a route stopped is offered again, and one not remembered failed.
const fn standing(remembered: &Remembered) -> Level {
    match remembered {
        Remembered::Advanced => Level::Info,
        Remembered::Kept => Level::Warn,
        Remembered::Failed(_) => Level::Error,
    }
}

/// What a report announced of its view's `pulls` open pull requests, its
/// events named as `printed` says.
fn reported(
    (source, printed): (&str, Printed),
    pulls: usize,
    announced: &[Fresh],
    cursor: &Remembered,
) -> Vec<Told> {
    let tail = match cursor {
        Remembered::Advanced => String::new(),
        Remembered::Kept => {
            "; a route stopped, so the next round reports the window again".to_owned()
        }
        Remembered::Failed(stop) => format!(
            "; the window is not remembered: {}. The next round reports it again",
            why(stop)
        ),
    };
    let window = announced
        .first()
        .map(|fresh| fresh.event.identity().as_str())
        .unwrap_or_default();
    let mut lines = vec![Told::at(
        standing(cursor),
        format!("{source}: {window} reports {pulls} open pull requests{tail}"),
    )];
    lines.extend(freshly((source, printed), announced));
    lines
}

/// What a poll of one source found and announced, as a log that may be
/// read in public says it: a private or internal repository's event named
/// only as private.
#[must_use]
pub fn polled(swept: &Swept) -> Vec<Told> {
    polled_as(swept, Printed::Withheld)
}

/// What a poll of one source found and announced, its events named as
/// `printed` says (ADR-0080).
#[must_use]
pub fn polled_as(swept: &Swept, printed: Printed) -> Vec<Told> {
    let source = match &swept.member {
        Some(member) => format!("{} {member}", swept.source.as_str()),
        None => swept.source.as_str().to_owned(),
    };
    match &swept.outcome {
        Sweep::Unchanged => vec![Told::info(format!(
            "{source}: unchanged since the last poll"
        ))],
        Sweep::Unkept(why) => vec![Told::warn(format!(
            "{source}: the installation token it reads with was minted but not kept, as {why}. This run reads with it; the next mints it again."
        ))],
        Sweep::Refused(maker) => vec![Told::warn(format!(
            "{source}: took no new release of {maker} this pull, as it keeps as many tombstones of its releases as peer_share lets it; the next pull reads them again"
        ))],
        Sweep::Ahead(maker) => vec![Told::warn(format!(
            "{source}: passed over an envelope of {maker} whose proof was made too far ahead of now"
        ))],
        Sweep::Stopped(stop) => vec![Told::error(format!(
            "{source}: stopped: {}. The next poll asks again.",
            why(stop)
        ))],
        Sweep::Listed {
            members,
            joined,
            left,
        } => {
            let named = |what: &str, paths: &[String]| {
                if paths.is_empty() {
                    String::new()
                } else {
                    format!("; {what}: {}", paths.join(", "))
                }
            };
            vec![Told::info(format!(
                "{source}: {members} members polled{}{}",
                named("joined", joined),
                named("left", left)
            ))]
        }
        Sweep::Polled {
            first,
            found,
            announced,
            cursor,
        } => {
            let new = announced.len();
            let head = if *first {
                format!("{source}: first poll found {found}; {new} announced, as backfill asks")
            } else {
                format!("{source}: {found} found, {new} new")
            };
            let tail = match cursor {
                Remembered::Advanced => String::new(),
                Remembered::Kept => {
                    "; a route stopped, so the next poll offers them again".to_owned()
                }
                Remembered::Failed(stop) => format!(
                    "; the cursor stays where it was: {}. The next poll offers them again",
                    why(stop)
                ),
            };
            let mut lines = vec![Told::at(standing(cursor), format!("{head}{tail}"))];
            lines.extend(freshly((&source, printed), announced));
            lines
        }
        Sweep::Reported {
            pulls,
            announced,
            cursor,
        } => reported((&source, printed), *pulls, announced, cursor),
        Sweep::Withdrawn {
            identity,
            announced,
        } => announced
            .iter()
            .map(|announced| {
                withdrew(identity, announced).reworded(|said| format!("{source}: {said}"))
            })
            .collect(),
        Sweep::Released {
            announced,
            withheld,
        } => {
            let mut lines: Vec<Told> = withheld
                .iter()
                .map(|why| {
                    Told::warn(format!(
                        "{source}: a held tag was let go unannounced, as {why}"
                    ))
                })
                .collect();
            lines.extend(freshly((&source, printed), announced));
            lines
        }
    }
}

/// What became of each of `announced`, events of `source` named as
/// `printed` says: held until its time, or what every route did with it.
fn freshly((source, printed): (&str, Printed), announced: &[Fresh]) -> Vec<Told> {
    let mut lines = Vec::new();
    for fresh in announced {
        let event = fresh.event.named_in(printed);
        if let Some(due) = fresh.held {
            lines.push(Told::info(format!(
                "{source} {event}: held for its Release until {due}"
            )));
        }
        lines.extend(fresh.announced.iter().map(|announced| {
            report(announced).reworded(|said| format!("{source} {event}: {said}"))
        }));
    }
    lines
}

/// What a route did withdrawing the release `identity` (ADR-0048).
#[must_use]
pub fn withdrew(identity: &Identity, announced: &Announced) -> Told {
    let identity = identity.as_str();
    let route = announced.route.as_str();
    match &announced.outcome {
        Outcome::Sent { steps: sent } if sent.is_empty() => Told::info(format!(
            "{route}: withdrew {identity}; nothing was left to delete"
        )),
        Outcome::Sent { steps: sent } => {
            Told::info(format!("{route}: withdrew {identity} ({})", steps(sent)))
        }
        _ => report(announced).reworded(|said| format!("withdrawing {identity}: {said}")),
    }
}

/// What one chore did.
#[must_use]
pub fn chored(kept: &HousekeptSchedule) -> Told {
    let Schedule {
        route,
        thread,
        chore,
        ..
    } = &kept.schedule;
    let route = route.as_str();
    let done = match chore {
        Chore::Archive => "archived",
        Chore::Lock => "locked",
    };
    match &kept.outcome {
        Chored::Done => Told::info(format!("{route}: thread {thread} {done}")),
        Chored::Dropped(reason) => Told::warn(format!(
            "{route}: thread {thread} not {done}, and not tried again: {reason}"
        )),
        Chored::Later(stop) => Told::error(format!(
            "{route}: thread {thread} not {done} yet: {}. The next run tries again.",
            why(stop)
        )),
    }
}

/// Why a run stopped.
#[must_use]
pub fn why(reason: &Stop) -> String {
    match reason {
        Stop::RetryAfter(after) => format!("the destination asked to wait {after:#}"),
        Stop::Refused(reason)
        | Stop::Unreachable(reason)
        | Stop::Unread(reason)
        | Stop::Store(reason)
        | Stop::Template(reason)
        | Stop::Marker(reason)
        | Stop::Proof(reason)
        | Stop::Source(reason)
        | Stop::RateLimited(reason)
        | Stop::Limit(reason) => reason.clone(),
    }
}

fn step(step: &Step) -> String {
    match step {
        Step::Post => "post".to_owned(),
        Step::Crosspost => "crosspost".to_owned(),
        Step::OpenThread => "thread".to_owned(),
        Step::Chunk(index) => format!("chunk {index}"),
        Step::Pin => "pin".to_owned(),
        Step::Unpin => "unpin".to_owned(),
        Step::Deliver(inbox) => format!("delivery to {inbox}"),
        Step::Deliveries => "deliveries to followers".to_owned(),
        Step::Edit { part: 0, .. } => "edit".to_owned(),
        Step::Edit { part, .. } => format!("edit of chunk {part}"),
        Step::Delete(0) => "deletion of the post".to_owned(),
        Step::Delete(index) => format!("deletion of chunk {index}"),
        Step::Rename(_) => "thread renamed".to_owned(),
        Step::Updates(_) => "updates to followers".to_owned(),
        Step::Deletes => "deletions to followers".to_owned(),
    }
}

/// The steps, named and listed.
#[must_use]
pub fn steps(steps: &[Step]) -> String {
    steps.iter().map(step).collect::<Vec<_>>().join(", ")
}

/// What one route did with an announcement.
#[must_use]
pub fn report(announced: &Announced) -> Told {
    let route = announced.route.as_str();
    let level = match &announced.outcome {
        Outcome::Sealed { stopped: true } | Outcome::Stopped { .. } => Level::Error,
        _ => Level::Info,
    };
    Told::at(level, reported_as(route, &announced.outcome))
}

/// What `route` did with an announcement whose outcome is `outcome`.
fn reported_as(route: &str, outcome: &Outcome) -> String {
    match outcome {
        Outcome::Sent { steps: sent } => format!("{route}: announced ({})", steps(sent)),
        Outcome::AlreadyAnnounced => format!("{route}: already announced; nothing sent"),
        Outcome::Sealed { stopped: false } => {
            format!("{route}: its private post is up to date; what it shows is not told")
        }
        Outcome::Sealed { stopped: true } => format!(
            "{route}: stopped before its private post was up to date; why is not told. The next run sends what remains."
        ),
        Outcome::Unedited => format!("{route}: an edit of nothing it announced; nothing sent"),
        Outcome::Skipped(reason) => format!("{route}: skipped, as {reason}; nothing sent"),
        Outcome::Gathered { key, due } => {
            format!("{route}: gathered into the group {key}, announced at {due}; nothing sent yet")
        }
        Outcome::Held { by, until } => {
            format!("{route}: run {by} holds it until {until}; nothing sent by this run")
        }
        Outcome::Stopped {
            landed,
            remaining,
            reason,
        } => {
            format!(
                "{route}: stopped: {}. Landed: {}. The next run sends what remains: {}.",
                why(reason),
                if landed.is_empty() {
                    "nothing".to_owned()
                } else {
                    steps(landed)
                },
                if remaining.is_empty() {
                    "nothing".to_owned()
                } else {
                    steps(remaining)
                },
            )
        }
    }
}

/// What a job did, and why the delivery it came by could not be released
/// where it could not.
#[must_use]
pub fn worked(worked: &Worked) -> Vec<Told> {
    let mut said = lines(&worked.did);
    said.extend(worked.unreleased.iter().map(|unreleased| {
        Told::warn(format!("{unreleased}; its sender's next delivery of it is refused until housekeeping forgets it"))
    }));
    said
}

/// What `did` is logged as, a line each.
fn lines(did: &Did) -> Vec<Told> {
    match did {
        Did::Announced(announced) => announced.iter().map(report).collect(),
        Did::Followed(followed) => vec![follow(followed)],
        Did::Interacted(interacted) => interacted.iter().map(answered).collect(),
        Did::Withheld(withheld) => vec![withheld_line(withheld)],
        Did::Polled(swept) => swept.iter().flat_map(polled).collect(),
        Did::Fanned(each) => each.iter().flat_map(lines).collect(),
        Did::Folded(Ok(moved)) => vec![Told::info(format!(
            "a webhook's change moved {moved} kept pull requests"
        ))],
        Did::Folded(Err(reason)) => vec![Told::error(format!(
            "a webhook's change could not be kept: {reason}"
        ))],
        Did::Stepped(stepped_queues) => stepped_queues.iter().flat_map(stepped).collect(),
    }
}

/// What an owner's webhook announcing nothing is logged as.
fn withheld_line(withheld: &Withheld) -> Told {
    let why = match withheld {
        Withheld::Unlisted => {
            "the owner's last membership record does not name its repository".to_owned()
        }
        Withheld::Left => "its repository left the owner's selection".to_owned(),
        Withheld::Unread(reason) => {
            format!("the owner's membership record could not be read: {reason}")
        }
        Withheld::Unnamed(reason) => format!("its project template names none: {reason}"),
        Withheld::Collides(reason) => reason.clone(),
    };
    Told::warn(format!("a webhook's release was not announced: {why}"))
}

/// What a job left undone as herald stopped is logged as, with why the
/// delivery it came by could not be released where it could not.
#[must_use]
pub fn unworked(job: &Job, released: &Result<(), String>) -> Told {
    let what = match job {
        Job::Announce { event, .. } | Job::App { event, .. } => {
            format!("the announcement of {}", event.identity().as_str())
        }
        Job::Follow { .. } => "an activity".to_owned(),
        Job::Poll { source, .. } => format!("the poll of {}", source.as_str()),
        Job::Interact { destination, .. } => {
            format!("the commands given on {}", destination.as_str())
        }
        Job::Fanned { jobs, .. } => format!("the {} jobs of one delivery", jobs.len()),
        Job::Fold { source, .. } => {
            format!("the change to {}'s kept pull requests", source.as_str())
        }
        Job::Pushed { source, .. } => format!("what a peer pushed into {}", source.as_str()),
        Job::Step { source, .. } => format!("the steps of {}'s merge queues", source.as_str()),
    };
    Told::warn(match released {
        Ok(()) => format!("herald stopped before it worked {what}"),
        Err(unreleased) => format!("herald stopped before it worked {what}; {unreleased}"),
    })
}

/// What a request refused for `reason` is logged as.
#[must_use]
pub fn refused(reason: &str) -> Told {
    Told::warn(format!("a request was refused: {reason}"))
}

/// What a record a listing skipped as corrupt, at `path`, asks of whoever
/// keeps the store.
#[must_use]
pub fn skipped(path: &str) -> Told {
    Told::warn(format!(
        "the record {path} could not be read and was skipped; fix or remove it"
    ))
}

/// What housekeeping did, or why it stopped.
#[must_use]
pub fn kept(kept: &Result<Housekept, Stop>) -> Vec<Told> {
    match kept {
        Ok(housekept) => housekept
            .kept
            .iter()
            .map(chored)
            .chain(housekept.skipped.iter().map(|path| skipped(path)))
            .chain(housekept.released.iter().flat_map(polled))
            .collect(),
        Err(stop) => vec![Told::error(format!(
            "housekeeping stopped: {}. The next round tries again.",
            why(stop)
        ))],
    }
}

fn answered(interacted: &Interacted) -> Told {
    let asked = &interacted.asked;
    let command = asked.command.name();
    let destination = asked.destination.as_str();
    match (&interacted.answer.trouble, &interacted.sent) {
        (Some(trouble), _) => Told::error(format!(
            "{destination}: {command} could not be answered: {trouble}"
        )),
        (None, Err(error)) => Told::error(format!(
            "{destination}: the answer to {command} could not be sent: {error}"
        )),
        (None, Ok(())) => Told::info(format!(
            "{destination}: {command} was answered in {}",
            asked.place.id
        )),
    }
}

fn follow(followed: &Result<Followed, FollowError>) -> Told {
    let level = match followed {
        Ok(Followed::Ignored(_)) => Level::Warn,
        Err(_) => Level::Error,
        Ok(_) => Level::Info,
    };
    Told::at(level, followed_as(followed))
}

/// What `followed` is logged as.
fn followed_as(followed: &Result<Followed, FollowError>) -> String {
    match followed {
        Ok(Followed::Accepted {
            follower, actor, ..
        }) => format!("{actor}: {follower} follows it now"),
        Ok(Followed::Waiting { follower, actor }) => format!(
            "{actor}: {follower} asks to follow it; approve or reject with `herald followers`"
        ),
        Ok(Followed::Rejected {
            follower, actor, ..
        }) => format!("{actor}: {follower} was turned away"),
        Ok(Followed::Undone { follower, actor }) => {
            format!("{actor}: {follower} no longer follows it")
        }
        Ok(Followed::Ignored(why)) => format!("an activity was ignored: {why}"),
        Err(error) => format!("an activity was not taken, and may be delivered again: {error}"),
    }
}

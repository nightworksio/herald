//! What a run did, in words: each announcement, poll, chore and job, as
//! every driver says it.

use herald_core::{Chore, Identity, Schedule, Step};
use herald_flows::{
    Announced, Chored, Did, FollowError, Followed, Fresh, Housekept, HousekeptSchedule, Interacted,
    Job, Outcome, Remembered, Stop, Sweep, Swept, Withheld, Worked,
};

/// What a report announced of its view's `pulls` open pull requests.
fn reported(source: &str, pulls: usize, announced: &[Fresh], cursor: &Remembered) -> Vec<String> {
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
    let mut lines = vec![format!(
        "{source}: {window} reports {pulls} open pull requests{tail}"
    )];
    lines.extend(freshly(source, announced));
    lines
}

/// What a poll of one source found and announced.
#[must_use]
pub fn polled(swept: &Swept) -> Vec<String> {
    let source = match &swept.member {
        Some(member) => format!("{} {member}", swept.source.as_str()),
        None => swept.source.as_str().to_owned(),
    };
    match &swept.outcome {
        Sweep::Unchanged => vec![format!("{source}: unchanged since the last poll")],
        Sweep::Unkept(why) => vec![format!(
            "{source}: the installation token it reads with was minted but not kept, as {why}. This run reads with it; the next mints it again."
        )],
        Sweep::Refused(maker) => vec![format!(
            "{source}: took no new release of {maker} this pull, as it keeps as many tombstones of its releases as peer_share lets it; the next pull reads them again"
        )],
        Sweep::Ahead(maker) => vec![format!(
            "{source}: passed over an envelope of {maker} whose proof was made too far ahead of now"
        )],
        Sweep::Stopped(stop) => vec![format!(
            "{source}: stopped: {}. The next poll asks again.",
            why(stop)
        )],
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
            vec![format!(
                "{source}: {members} members polled{}{}",
                named("joined", joined),
                named("left", left)
            )]
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
            let mut lines = vec![format!("{head}{tail}")];
            lines.extend(freshly(&source, announced));
            lines
        }
        Sweep::Reported {
            pulls,
            announced,
            cursor,
        } => reported(&source, *pulls, announced, cursor),
        Sweep::Withdrawn {
            identity,
            announced,
        } => announced
            .iter()
            .map(|announced| format!("{source}: {}", withdrew(identity, announced)))
            .collect(),
        Sweep::Released {
            announced,
            withheld,
        } => {
            let mut lines: Vec<String> = withheld
                .iter()
                .map(|why| format!("{source}: a held tag was let go unannounced, as {why}"))
                .collect();
            lines.extend(freshly(&source, announced));
            lines
        }
    }
}

/// What became of each of `announced`, events of `source`: held until its
/// time, or what every route did with it.
fn freshly(source: &str, announced: &[Fresh]) -> Vec<String> {
    let mut lines = Vec::new();
    for fresh in announced {
        let event = fresh.event.version().unwrap_or(fresh.event.title());
        if let Some(due) = fresh.held {
            lines.push(format!(
                "{source} {event}: held for its Release until {due}"
            ));
        }
        lines.extend(
            fresh
                .announced
                .iter()
                .map(|announced| format!("{source} {event}: {}", report(announced))),
        );
    }
    lines
}

/// What a route did withdrawing the release `identity` (ADR-0048).
#[must_use]
pub fn withdrew(identity: &Identity, announced: &Announced) -> String {
    let identity = identity.as_str();
    let route = announced.route.as_str();
    match &announced.outcome {
        Outcome::Sent { steps: sent } if sent.is_empty() => {
            format!("{route}: withdrew {identity}; nothing was left to delete")
        }
        Outcome::Sent { steps: sent } => format!("{route}: withdrew {identity} ({})", steps(sent)),
        _ => format!("withdrawing {identity}: {}", report(announced)),
    }
}

/// What one chore did.
#[must_use]
pub fn chored(kept: &HousekeptSchedule) -> String {
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
        Chored::Done => format!("{route}: thread {thread} {done}"),
        Chored::Dropped(reason) => {
            format!("{route}: thread {thread} not {done}, and not tried again: {reason}")
        }
        Chored::Later(stop) => format!(
            "{route}: thread {thread} not {done} yet: {}. The next run tries again.",
            why(stop)
        ),
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
pub fn report(announced: &Announced) -> String {
    let route = announced.route.as_str();
    match &announced.outcome {
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
pub fn worked(worked: &Worked) -> Vec<String> {
    let mut said = lines(&worked.did);
    said.extend(worked.unreleased.iter().map(|unreleased| {
        format!("{unreleased}; its sender's next delivery of it is refused until housekeeping forgets it")
    }));
    said
}

/// What `did` is logged as, a line each.
fn lines(did: &Did) -> Vec<String> {
    match did {
        Did::Announced(announced) => announced.iter().map(report).collect(),
        Did::Followed(followed) => vec![follow(followed)],
        Did::Interacted(interacted) => interacted.iter().map(answered).collect(),
        Did::Withheld(withheld) => vec![withheld_line(withheld)],
        Did::Polled(swept) => swept.iter().flat_map(polled).collect(),
        Did::Fanned(each) => each.iter().flat_map(lines).collect(),
        Did::Folded(Ok(moved)) => vec![format!(
            "a webhook's change moved {moved} kept pull requests"
        )],
        Did::Folded(Err(reason)) => vec![format!("a webhook's change could not be kept: {reason}")],
    }
}

/// What an owner's webhook announcing nothing is logged as.
fn withheld_line(withheld: &Withheld) -> String {
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
    format!("a webhook's release was not announced: {why}")
}

/// What a job left undone as herald stopped is logged as, with why the
/// delivery it came by could not be released where it could not.
#[must_use]
pub fn unworked(job: &Job, released: &Result<(), String>) -> String {
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
    };
    match released {
        Ok(()) => format!("herald stopped before it worked {what}"),
        Err(unreleased) => format!("herald stopped before it worked {what}; {unreleased}"),
    }
}

/// What a request refused for `reason` is logged as.
#[must_use]
pub fn refused(reason: &str) -> String {
    format!("a request was refused: {reason}")
}

/// What a record a listing skipped as corrupt, at `path`, asks of whoever
/// keeps the store.
#[must_use]
pub fn skipped(path: &str) -> String {
    format!("the record {path} could not be read and was skipped; fix or remove it")
}

/// What housekeeping did, or why it stopped.
#[must_use]
pub fn kept(kept: &Result<Housekept, Stop>) -> Vec<String> {
    match kept {
        Ok(housekept) => housekept
            .kept
            .iter()
            .map(chored)
            .chain(housekept.skipped.iter().map(|path| skipped(path)))
            .chain(housekept.released.iter().flat_map(polled))
            .collect(),
        Err(stop) => vec![format!(
            "housekeeping stopped: {}. The next round tries again.",
            why(stop)
        )],
    }
}

fn answered(interacted: &Interacted) -> String {
    let asked = &interacted.asked;
    let command = asked.command.name();
    let destination = asked.destination.as_str();
    match (&interacted.answer.trouble, &interacted.sent) {
        (Some(trouble), _) => format!("{destination}: {command} could not be answered: {trouble}"),
        (None, Err(error)) => {
            format!("{destination}: the answer to {command} could not be sent: {error}")
        }
        (None, Ok(())) => format!(
            "{destination}: {command} was answered in {}",
            asked.place.id
        ),
    }
}

fn follow(followed: &Result<Followed, FollowError>) -> String {
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

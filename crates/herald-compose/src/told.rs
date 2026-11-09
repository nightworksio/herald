//! What a run did, in words: each announcement, poll, chore and job, as
//! every driver says it.

use herald_core::{Chore, Schedule, Step};
use herald_flows::{
    Announced, Chored, FollowError, Followed, Housekept, Interacted, Kept, Outcome, Remembered,
    Stop, Sweep, Swept, Worked,
};

/// What a poll of one source found and announced.
#[must_use]
pub fn polled(swept: &Swept) -> Vec<String> {
    let source = swept.source.as_str();
    match &swept.outcome {
        Sweep::Unchanged => vec![format!("{source}: unchanged since the last poll")],
        Sweep::Stopped(stop) => vec![format!(
            "{source}: stopped: {}. The next poll asks again.",
            why(stop)
        )],
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
            for fresh in announced {
                let event = fresh.event.version().unwrap_or(fresh.event.title());
                lines.extend(
                    fresh
                        .announced
                        .iter()
                        .map(|announced| format!("{source} {event}: {}", report(announced))),
                );
            }
            lines
        }
    }
}

/// What one chore did.
#[must_use]
pub fn chored(kept: &Kept) -> String {
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
        | Stop::Store(reason)
        | Stop::Template(reason)
        | Stop::Marker(reason)
        | Stop::Source(reason) => reason.clone(),
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

/// What a job did.
#[must_use]
pub fn worked(worked: &Worked) -> Vec<String> {
    match worked {
        Worked::Announced(announced) => announced.iter().map(report).collect(),
        Worked::Followed(followed) => vec![follow(followed)],
        Worked::Interacted(interacted) => interacted.iter().map(answered).collect(),
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

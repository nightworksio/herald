//! When a rule alerts on a pull request that left its forge's merge queue
//! for one of the reasons the rule names, once per time it left
//! (ADR-0078).

use std::collections::BTreeMap;

use crate::event::{Draft, Event, Problem};
use crate::kind::{
    AUTHOR, Attribute, BOT, FAILURE, Kind, LEFT, NUMBER, PULL_LINK, REASON, REPOSITORY, RULE,
};
use crate::nudge::{NUDGE_PREFIX, pull_identity};
use crate::queue::{Board, Dequeued, Left, Pull};

/// A pull request a rule alerts on, and when and why it left the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dequeue<'a> {
    /// The pull request.
    pub pull: &'a Pull,
    /// When and why it left.
    pub left: Left,
}

/// Each pull request of `board` whose entry left the queue for one of
/// `reasons`, the latest to leave first, at most `most`.
#[must_use]
pub fn dequeues<'a>(board: &'a Board, reasons: &[Dequeued], most: usize) -> Vec<Dequeue<'a>> {
    let mut left: Vec<Dequeue<'a>> = board
        .distinct_rows()
        .into_iter()
        .filter_map(|row| {
            let left = row.pull.queue.as_ref()?.left?;
            reasons.contains(&left.reason).then_some(Dequeue {
                pull: &row.pull,
                left,
            })
        })
        .collect();
    left.sort_by(|first, second| {
        second
            .left
            .at
            .cmp(&first.left.at)
            .then_with(|| pull_identity(first.pull).cmp(&pull_identity(second.pull)))
    });
    left.truncate(most);
    left
}

/// The identity of rule `rule`'s alert on `dequeue`.
#[must_use]
pub fn dequeue_identity(rule: &str, dequeue: Dequeue<'_>) -> String {
    format!(
        "{NUDGE_PREFIX}/{rule}/{}/dequeued/{}",
        pull_identity(dequeue.pull),
        dequeue.left.at
    )
}

/// The event of rule `rule`'s alert on `dequeue`, its project the pull
/// request's, seen as its repository is.
///
/// # Errors
///
/// Where the pull request's project is empty, or its title holds nothing
/// but space.
pub fn dequeue_event(rule: &str, dequeue: Dequeue<'_>) -> Result<Event, Problem> {
    let pull = dequeue.pull;
    let text = |text: &str| Attribute::Text(text.to_owned());
    let reason = dequeue.left.reason;
    let mut attributes = BTreeMap::from([
        (RULE.to_owned(), text(rule)),
        (REASON.to_owned(), text(reason.name())),
        (FAILURE.to_owned(), Attribute::Flag(reason.failure())),
        (LEFT.to_owned(), text(&dequeue.left.at.to_string())),
        (REPOSITORY.to_owned(), text(&pull.repository)),
        (NUMBER.to_owned(), Attribute::Number(pull.number.into())),
        (BOT.to_owned(), Attribute::Flag(pull.bot)),
    ]);
    if let Some(author) = &pull.author {
        attributes.insert(AUTHOR.to_owned(), text(author));
    }
    let event = Event::normalise(Draft {
        identity: dequeue_identity(rule, dequeue),
        kind: Kind::DEQUEUED,
        project: pull.project.clone(),
        title: pull.title.clone(),
        body: String::new(),
        links: BTreeMap::from([(PULL_LINK.to_owned(), pull.url.clone())]),
        attributes,
        occurred: dequeue.left.at,
    })?;
    Ok(event.seen_as(pull.visibility))
}

#[cfg(test)]
mod tests;

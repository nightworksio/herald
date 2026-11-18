//! When a rule nudges: an open pull request a view shows that waited past
//! one of the rule's stages, once per stage and spell of waiting (ADR-0056).

use std::collections::BTreeMap;

use jiff::SignedDuration;

use crate::address::Address;
use crate::event::{Draft, Event, Problem};
use crate::kind::{
    AUTHOR, Attribute, BOT, Kind, NUMBER, PULL_LINK, REASON, REPOSITORY, RULE, SPELL, STAGE, TURN,
    WAITED, WAITING_ON,
};
use crate::queue::{Board, Pull, Row};
use crate::render::waited;

/// What an event's identity begins with where it is a nudge.
pub const NUDGE_PREFIX: &str = "rule";

/// The most pull requests a rule nudges a window where it names no `most`;
/// the rest wait for the next window.
pub const MOST_NUDGES: usize = 20;

/// The most stages a rule names.
pub const MOST_STAGES: usize = 16;

/// The units a stage is written in, the largest first.
const UNITS: [(i64, &str); 5] = [
    (7 * 24 * 3_600, "w"),
    (24 * 3_600, "d"),
    (3_600, "h"),
    (60, "m"),
    (1, "s"),
];

/// How long a pull request waits before each of a rule's nudges: at least
/// one, each longer than the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stages(Vec<SignedDuration>);

impl Stages {
    /// `stages`, shortest first, where there are from one to
    /// [`MOST_STAGES`], each a whole second or more and none twice.
    #[must_use]
    pub fn new(mut stages: Vec<SignedDuration>) -> Option<Self> {
        stages.sort_unstable();
        let distinct = stages
            .iter()
            .zip(stages.iter().skip(1))
            .all(|(shorter, longer)| shorter < longer);
        let whole = stages
            .iter()
            .all(|stage| stage.as_secs() > 0 && stage.subsec_nanos() == 0);
        (distinct && whole && (1..=MOST_STAGES).contains(&stages.len())).then_some(Self(stages))
    }

    /// The stages, shortest first.
    #[must_use]
    pub fn as_slice(&self) -> &[SignedDuration] {
        &self.0
    }

    /// The longest stage `waited` reached, where it reached one.
    #[must_use]
    pub fn reached(&self, waited: SignedDuration) -> Option<SignedDuration> {
        self.0.iter().rev().find(|stage| **stage <= waited).copied()
    }
}

/// `stage` as a rule's identity and its nudge write it, in its largest
/// whole unit: `2d`, `36h`, `90s`.
#[must_use]
pub fn stage_name(stage: SignedDuration) -> String {
    let seconds = stage.as_secs();
    let (unit, name) = UNITS
        .into_iter()
        .find(|(unit, _)| seconds % unit == 0)
        .unwrap_or((1, "s"));
    format!("{}{name}", seconds / unit)
}

/// What names `pull` across every forge: its page's host, its
/// repository's full path and its number; its project in place of the
/// host where its page names none.
#[must_use]
pub fn pull_identity(pull: &Pull) -> String {
    let host = Address::parse(&pull.url)
        .map_or_else(|| pull.project.clone(), |page| page.host().to_owned());
    format!("{host}/{}/pull/{}", pull.repository, pull.number)
}

/// The identity of rule `rule`'s nudge of `pull` at `stage`, in the spell
/// of waiting that began when anyone last acted on it.
#[must_use]
pub fn nudge_identity(rule: &str, pull: &Pull, stage: SignedDuration) -> String {
    format!(
        "{NUDGE_PREFIX}/{rule}/{}/{}/{}",
        pull_identity(pull),
        pull.updated,
        stage_name(stage)
    )
}

/// A row a rule nudges, at the longest stage it reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nudge<'a> {
    /// The row.
    pub row: &'a Row,
    /// The stage.
    pub stage: SignedDuration,
}

/// Each row of `board` that reached one of `stages`, at the longest it
/// reached, the longest waiting first, at most `most`.
#[must_use]
pub fn nudges<'a>(board: &'a Board, stages: &Stages, most: usize) -> Vec<Nudge<'a>> {
    let mut nudged: Vec<Nudge<'a>> = board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .chain(board.batches.iter().flat_map(|batch| &batch.rows))
        .filter_map(|row| stages.reached(row.waited).map(|stage| Nudge { row, stage }))
        .collect();
    nudged.sort_by(|left, right| {
        right
            .row
            .waited
            .cmp(&left.row.waited)
            .then_with(|| pull_identity(&left.row.pull).cmp(&pull_identity(&right.row.pull)))
    });
    nudged.truncate(most);
    nudged
}

/// The event of rule `rule`'s `nudge`, its project the pull request's,
/// seen as its repository is.
///
/// # Errors
///
/// Where the pull request's project is empty, or its title holds nothing
/// but space.
pub fn nudge_event(rule: &str, nudge: Nudge<'_>) -> Result<Event, Problem> {
    let row = nudge.row;
    let pull = &row.pull;
    let text = |text: &str| Attribute::Text(text.to_owned());
    let mut attributes = BTreeMap::from([
        (RULE.to_owned(), text(rule)),
        (STAGE.to_owned(), text(&stage_name(nudge.stage))),
        (TURN.to_owned(), text(row.state.name())),
        (REASON.to_owned(), text(&row.reason.written())),
        (WAITED.to_owned(), text(&waited(row.waited))),
        (SPELL.to_owned(), text(&pull.updated.to_string())),
        (REPOSITORY.to_owned(), text(&pull.repository)),
        (NUMBER.to_owned(), Attribute::Number(pull.number.into())),
        (BOT.to_owned(), Attribute::Flag(pull.bot)),
    ]);
    if let Some(author) = &pull.author {
        attributes.insert(AUTHOR.to_owned(), text(author));
    }
    let waiting = row.waiting_on();
    if !waiting.is_empty() {
        attributes.insert(WAITING_ON.to_owned(), text(&waiting.join(", ")));
    }
    let event = Event::normalise(Draft {
        identity: nudge_identity(rule, pull, nudge.stage),
        kind: Kind::NUDGE,
        project: pull.project.clone(),
        title: pull.title.clone(),
        body: String::new(),
        links: BTreeMap::from([(PULL_LINK.to_owned(), pull.url.clone())]),
        attributes,
        occurred: pull
            .updated
            .checked_add(nudge.stage)
            .unwrap_or(pull.updated),
    })?;
    Ok(event.seen_as(pull.visibility))
}

#[cfg(test)]
mod tests;

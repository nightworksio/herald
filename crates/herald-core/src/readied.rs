//! When a rule alerts on its view's release, for one audience of its
//! routes at a time: once the release turns ready as that audience is told
//! it, and once a milestone that audience may see nears its due day while
//! the release is still not ready (ADR-0085).

use std::collections::BTreeMap;

use jiff::civil::Date;
use jiff::{Timestamp, Unit};
use serde::{Deserialize, Serialize};

use crate::event::{Draft, Event, Problem};
use crate::kind::{
    Attribute, DAYS, DUE, HOLDS, Kind, MILESTONE_LINK, MILESTONE_TITLE, RULE, VERDICT, VIEW,
};
use crate::members::Visibility;
use crate::nudge::NUDGE_PREFIX;
use crate::queue::{Based, Beside, Header, OpenMilestone, Readiness, Row, View, grouped};

/// The most days before a milestone's due day a rule alerts on a release
/// still not ready.
pub const MOST_LATE_DAYS: u16 = 365;

/// What the `verdict` of an alert on a release that turned ready says.
pub const READY_VERDICT: &str = "ready";

/// What the `verdict` of an alert on a release still not ready as a
/// milestone's due day nears says.
pub const LATE_VERDICT: &str = "late";

/// What a rule alerts on of its view's release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Alerting {
    /// Whether it alerts once the release turns ready.
    pub ready: bool,
    /// How many days before a milestone's due day it alerts on the
    /// release still not ready, where it does.
    pub late: Option<u16>,
}

/// A release's verdict as one audience was told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseVerdict {
    /// Nothing holds it back.
    Ready,
    /// Something holds it back.
    Held,
    /// Whether anything holds it back is not told.
    Unknown,
}

impl ReleaseVerdict {
    /// The verdict `readiness` tells.
    #[must_use]
    pub fn of(readiness: &Readiness) -> Self {
        match readiness.ready() {
            Some(true) => Self::Ready,
            Some(false) => Self::Held,
            None => Self::Unknown,
        }
    }
}

/// What one audience of a rule's routes is told of its view's release.
#[derive(Debug, Clone, Copy)]
pub struct Told<'a> {
    /// Who the audience may see.
    pub audience: Visibility,
    /// The verdict it is told.
    pub readiness: &'a Readiness,
    /// The release's header as it is shown it, where one was read.
    pub header: Option<&'a Header>,
}

/// What the board of `view` tells the audience that may see `audience` of
/// its release, `shown` its rows beside the rows `withheld` from it: the
/// verdict judged beside those and the base branches of `bases` it may
/// see, and of `header` only what it may see; none where the view has no
/// release.
#[must_use]
pub fn told_of(
    view: &View,
    (shown, withheld): (Vec<Row>, &[Row]),
    (bases, header): (&[Based], Option<&Header>),
    audience: Visibility,
) -> Option<(Readiness, Option<Header>)> {
    let beside = Beside {
        withheld,
        bases,
        sees: audience,
    };
    let readiness = grouped(view, shown, beside).readiness?;
    let header = header.map(|header| header.shown(|seen| seen <= audience));
    Some((readiness, header))
}

/// One alert of a rule on its view's release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readied<'a> {
    /// The release turned ready.
    Ready,
    /// The release is still not ready, `days` before `milestone`'s due day,
    /// or that many days after where `days` is below zero.
    Late {
        /// The milestone due first of those the audience may see.
        milestone: &'a OpenMilestone,
        /// The days until its due day.
        days: i64,
    },
}

/// The milestone of `header` due first, where any names a due day.
fn due_first(header: &Header) -> Option<(&OpenMilestone, Date)> {
    header
        .milestones
        .iter()
        .filter_map(|milestone| milestone.due.map(|due| (milestone, due)))
        .min_by(|(left, left_due), (right, right_due)| {
            (left_due, &left.repository, &left.title).cmp(&(
                right_due,
                &right.repository,
                &right.title,
            ))
        })
}

/// What `alerting` alerts on of what `told` tells its audience on `today`,
/// the audience told `last` before, where it was told any: the release
/// turned ready from another verdict told before; and still not ready,
/// known held, on or after `late` days before the due day of the
/// milestone it may see due first.
#[must_use]
pub fn readied(
    alerting: Alerting,
    told: Told<'_>,
    last: Option<ReleaseVerdict>,
    today: Date,
) -> Vec<Readied<'_>> {
    let verdict = ReleaseVerdict::of(told.readiness);
    let mut alerts = Vec::new();
    if alerting.ready
        && verdict == ReleaseVerdict::Ready
        && last.is_some_and(|last| last != ReleaseVerdict::Ready)
    {
        alerts.push(Readied::Ready);
    }
    let late = alerting.late.filter(|_| verdict == ReleaseVerdict::Held);
    if let Some((days, (milestone, due))) = late.zip(told.header.and_then(due_first)) {
        let left = today
            .until((Unit::Day, due))
            .map_or(i64::MIN, |span| i64::from(span.get_days()));
        if left <= i64::from(days) {
            alerts.push(Readied::Late {
                milestone,
                days: left,
            });
        }
    }
    alerts
}

/// The identity of rule `rule`'s alert `readied` to the audience that may
/// see `audience`: a turn to ready told once for each verdict `since`, the
/// identity of the window its cursor last remembered; a late release once
/// for each milestone title and due day.
#[must_use]
pub fn readied_identity(
    rule: &str,
    audience: Visibility,
    readied: Readied<'_>,
    since: &str,
) -> String {
    let audience = audience.name();
    match readied {
        Readied::Ready => format!("{NUDGE_PREFIX}/{rule}/release/{audience}/ready/{since}"),
        Readied::Late { milestone, .. } => {
            let due = milestone.due.map(|due| due.to_string()).unwrap_or_default();
            format!(
                "{NUDGE_PREFIX}/{rule}/release/{audience}/late/{}/{due}",
                milestone.title
            )
        }
    }
}

/// The event of rule `rule`'s alert `readied` on the release of the view
/// `view`, as `told` tells its audience, its project the view's name, seen
/// as the audience is, occurred at `now`.
///
/// # Errors
///
/// Where the view's name is empty.
pub fn readied_event(
    (rule, view): (&str, &str),
    told: Told<'_>,
    readied: Readied<'_>,
    (since, now): (&str, Timestamp),
) -> Result<Event, Problem> {
    let text = |text: &str| Attribute::Text(text.to_owned());
    let holds = told.readiness.holds.len() + told.readiness.more;
    let mut attributes = BTreeMap::from([
        (RULE.to_owned(), text(rule)),
        (VIEW.to_owned(), text(view)),
        (
            HOLDS.to_owned(),
            Attribute::Number(u64::try_from(holds).unwrap_or(u64::MAX).into()),
        ),
    ]);
    let mut links = BTreeMap::new();
    match readied {
        Readied::Ready => {
            attributes.insert(VERDICT.to_owned(), text(READY_VERDICT));
        }
        Readied::Late { milestone, days } => {
            attributes.insert(VERDICT.to_owned(), text(LATE_VERDICT));
            attributes.insert(MILESTONE_TITLE.to_owned(), text(&milestone.title));
            attributes.insert(DAYS.to_owned(), Attribute::Number(days.into()));
            if let Some(due) = milestone.due {
                attributes.insert(DUE.to_owned(), text(&due.to_string()));
            }
            if let Some(url) = &milestone.url {
                links.insert(MILESTONE_LINK.to_owned(), url.clone());
            }
        }
    }
    let event = Event::normalise(Draft {
        identity: readied_identity(rule, told.audience, readied, since),
        kind: Kind::READINESS,
        project: view.to_owned(),
        title: view.to_owned(),
        body: String::new(),
        links,
        attributes,
        occurred: now,
    })?;
    Ok(event.seen_as(told.audience))
}

#[cfg(test)]
mod tests;

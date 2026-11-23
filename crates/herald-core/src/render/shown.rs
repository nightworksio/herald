//! A report's view as a template reads it: every key named here, and no
//! other.

use jiff::SignedDuration;
use minijinja::{Value, context};

use std::collections::BTreeMap;

use crate::queue::{
    Advisories, Advisory, BaseRead, Based, Batch, Checks, Failure, Grouped, Header, Held, Holder,
    Loaded, Loads, Overviews, QueueEntry, Readiness, Row, Running, Showing, capped,
};

/// How long `waited` is, in its largest whole unit: days, hours or
/// minutes, as a report and `herald queue` write it.
#[must_use]
pub fn waited(waited: SignedDuration) -> String {
    let minutes = waited.as_mins();
    match (minutes / (24 * 60), minutes / 60) {
        (0, 0) => format!("{minutes}m"),
        (0, hours) => format!("{hours}h"),
        (days, _) => format!("{days}d"),
    }
}

/// A pull request's entry in its forge's merge queue, as a row shows it
/// (ADR-0078).
fn queue_value(entry: &QueueEntry) -> Value {
    context! {
        position => entry.position,
        state => entry.state.name(),
        since => entry.since.map(|at| at.to_string()),
        estimate_seconds => entry.estimate,
        left => entry.left.map(|left| context! {
            at => left.at.to_string(),
            reason => left.reason.name(),
            failure => left.reason.failure(),
        }),
    }
}

fn row_value(row: &Row) -> Value {
    let pull = &row.pull;
    context! {
        project => pull.project.as_str(),
        repository => pull.repository.as_str(),
        number => pull.number,
        title => pull.title.as_str(),
        url => pull.url.as_str(),
        author => pull.author.as_deref(),
        bot => pull.bot,
        draft => pull.draft,
        labels => pull.labels.clone(),
        milestone => pull.milestone.as_deref(),
        base => pull.base.as_deref(),
        head_branch => pull.head_branch.as_deref(),
        reviewers => pull.reviewers.clone(),
        waiting_on => row.waiting_on().into_iter().map(str::to_owned).collect::<Vec<_>>(),
        opened => pull.opened.to_string(),
        updated => pull.updated.to_string(),
        state => row.state.name(),
        reason => row.reason.written(),
        waited => waited(row.waited),
        waited_seconds => row.waited.as_secs(),
        stale => row.stale,
        overlaps => row.overlaps.clone(),
        areas => pull
            .areas
            .iter()
            .map(|area| context! { name => area.name.as_str(), kind => area.kind.name() })
            .collect::<Vec<_>>(),
        queue => pull.queue.as_ref().map(queue_value),
        decided => row
            .decided
            .iter()
            .map(|(slot, choice)| (slot.name(), choice.clone()))
            .collect::<std::collections::BTreeMap<_, _>>(),
    }
}

fn rows_value(rows: &[Row]) -> Vec<Value> {
    rows.iter().map(row_value).collect()
}

fn group_value(group: &Grouped) -> Value {
    context! {
        name => group.name.as_str(),
        rows => rows_value(&group.rows),
    }
}

fn batch_value(batch: &Batch) -> Value {
    context! {
        package => batch.bump.as_ref().map(|bump| bump.package.clone()),
        to => batch.bump.as_ref().and_then(|bump| bump.to.clone()),
        green => batch.green,
        failing => batch.failing,
        rows => rows_value(&batch.rows),
    }
}

fn failure_value(failure: &Failure) -> Value {
    context! {
        check => failure.check.as_str(),
        count => failure.pulls.len(),
        broken => failure.broken,
        flips => failure.flips,
        flaky => failure.flaky,
        pulls => failure
            .pulls
            .iter()
            .map(|(repository, number)| context! { repository => repository, number => number })
            .collect::<Vec<_>>(),
    }
}

fn run_value(run: &Running) -> Value {
    context! {
        repository => run.repository.as_str(),
        id => run.id.as_str(),
        url => run.url.as_str(),
        sha => run.sha.as_str(),
        created => run.created.to_string(),
    }
}

fn unread_value(unread: &crate::queue::Unread) -> Value {
    context! {
        repository => unread.repository.as_str(),
        reason => unread.reason.as_str(),
    }
}

fn advisory_value(advisory: &Advisory, at: jiff::Timestamp) -> Value {
    context! {
        repository => advisory.repository.as_str(),
        tool => advisory.tool.as_str(),
        severity => advisory.severity.as_deref(),
        summary => advisory.summary.as_deref(),
        state => advisory.state.as_str(),
        url => advisory.url.as_str(),
        opened => advisory.opened.to_string(),
        age => waited(at.duration_since(advisory.opened)),
    }
}

/// The advisories section as a private surface is shown it (ADR-0067).
fn advisories_value(found: &Advisories) -> Value {
    context! {
        count => found.open.len(),
        more => found.more,
        open => found.open.iter().map(|advisory| advisory_value(advisory, found.at)).collect::<Vec<_>>(),
        unread => found.unread.iter().map(unread_value).collect::<Vec<_>>(),
    }
}

/// What one team, area or "other" counts: `reviewed` absent where the
/// reviews given were not counted.
fn loaded_value(count: Loaded, reviewed: bool) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([
        ("requested", Value::from(count.requested)),
        ("reviewed", Value::from(reviewed.then_some(count.reviewed))),
    ])
}

/// The load section: each team and area shown by name, and "other"
/// (ADR-0075).
fn load_value(loads: &Loads) -> Value {
    let teams: Vec<Value> = loads
        .teams
        .iter()
        .map(|(name, count)| {
            let mut keys = loaded_value(*count, loads.reviewed);
            keys.insert("name", Value::from(name.as_str()));
            Value::from(keys)
        })
        .collect();
    let areas: Vec<Value> = loads
        .areas
        .iter()
        .map(|(area, count)| {
            let mut keys = loaded_value(*count, loads.reviewed);
            keys.insert("name", Value::from(area.name.as_str()));
            keys.insert("kind", Value::from(area.kind.name()));
            Value::from(keys)
        })
        .collect();
    context! {
        teams => teams,
        areas => areas,
        other => loads.other.map(|count| Value::from(loaded_value(count, loads.reviewed))),
    }
}

/// The overviews `overviews` holds, `capacity` and `load` absent where the
/// view shows none (ADR-0066, ADR-0075); `advisories` present only where
/// its surface was
/// shown them, and `withheld`, the one line said in their place, only
/// where it was not (ADR-0067).
fn overviews_value(overviews: &Overviews) -> Value {
    let capacity = overviews.capacity.as_ref().map(|runs| {
        let queued: Vec<&Running> = runs.iter().filter(|run| run.queued).collect();
        context! {
            queued => queued.len(),
            running => runs.len() - queued.len(),
            oldest_queued => queued.iter().map(|run| run.created).min().map(|at| at.to_string()),
            superseded => overviews.superseded().into_iter().map(run_value).collect::<Vec<_>>(),
        }
    });
    let unread: Vec<Value> = overviews.unread.iter().map(unread_value).collect();
    let mut keys = BTreeMap::from([
        ("capacity", Value::from(capacity)),
        ("unread", Value::from(unread)),
        ("load", Value::from(overviews.load.as_ref().map(load_value))),
        (
            "header",
            Value::from(overviews.header.as_ref().map(header_value)),
        ),
        (
            "bases",
            Value::from(overviews.bases.iter().map(based_value).collect::<Vec<_>>()),
        ),
    ]);
    match overviews.advisories.as_ref().map(|advised| advised.shown()) {
        Some(Some(found)) => {
            keys.insert("advisories", advisories_value(found));
        }
        Some(None) => {
            keys.insert("withheld", Value::from(crate::queue::WITHHELD));
        }
        None => {}
    }
    Value::from(keys)
}

/// A release's header: each open milestone its selector names and each
/// repository whose milestones could not be read (ADR-0082).
fn header_value(header: &Header) -> Value {
    let milestones: Vec<Value> = header
        .milestones
        .iter()
        .map(|milestone| {
            context! {
                repository => milestone.repository.as_str(),
                title => milestone.title.as_str(),
                url => milestone.url.as_deref(),
                due => milestone.due.map(|due| due.to_string()),
                overdue => milestone.overdue,
                open => milestone.counts.map(|(open, _)| open),
                closed => milestone.counts.map(|(_, closed)| closed),
            }
        })
        .collect();
    context! {
        milestones => milestones,
        unread => header.unread.iter().map(unread_value).collect::<Vec<_>>(),
    }
}

/// One base branch of a release and its head's checks: `failing` naming at
/// most [`crate::queue::MOST_HELD_CHECKS`], `reason` why it was not read
/// (ADR-0083).
fn based_value(based: &Based) -> Value {
    let (state, failing, reason) = match &based.read {
        BaseRead::Checked(Checks::Absent) => ("absent", Vec::new(), None),
        BaseRead::Checked(Checks::Pending) => ("pending", Vec::new(), None),
        BaseRead::Checked(Checks::Passed) => ("passed", Vec::new(), None),
        BaseRead::Checked(Checks::Failed(names)) => ("failed", names.clone(), None),
        BaseRead::Unread(reason) => ("unread", Vec::new(), Some(reason.as_str())),
    };
    let (failing, more) = capped(failing);
    context! {
        repository => based.repository.as_str(),
        branch => based.branch.as_str(),
        state => state,
        failing => failing,
        more => more,
        reason => reason,
    }
}

/// One hold on a release: a private one naming nothing of its pull
/// request or base branch (ADR-0081, ADR-0083).
fn held_value(held: &Held) -> Value {
    match held {
        Held::Shown(holding) => context! {
            private => false,
            reason => holding.held_by.name(),
            repository => holding.repository.as_str(),
            number => match &holding.holder {
                Holder::Pull { number, .. } => Some(*number),
                Holder::Base { .. } => None,
            },
            url => match &holding.holder {
                Holder::Pull { url, .. } => Some(url.as_str()),
                Holder::Base { .. } => None,
            },
            branch => match &holding.holder {
                Holder::Pull { .. } => None,
                Holder::Base { branch } => Some(branch.as_str()),
            },
            named => holding.named.clone(),
            more => holding.more,
        },
        Held::Private => context! {
            private => true,
            reason => Value::from(()),
            repository => Value::from(()),
            number => Value::from(()),
            url => Value::from(()),
            branch => Value::from(()),
            named => Vec::<String>::new(),
            more => 0,
        },
    }
}

/// Whether a release is ready, none where that is not known, and each
/// pull request holding it back (ADR-0081).
fn readiness_value(readiness: &Readiness) -> Value {
    context! {
        ready => readiness.ready(),
        holds => readiness.holds.iter().map(held_value).collect::<Vec<_>>(),
        more => readiness.more,
    }
}

/// `showing` as a template reads it as `view`.
pub(super) fn view_value(showing: Showing<'_>) -> Value {
    let board = showing.board;
    context! {
        name => showing.view,
        total => board.total(),
        stale => board.stale,
        counts => board
            .counts
            .iter()
            .map(|(state, count)| context! { state => state.name(), count => count })
            .collect::<Vec<_>>(),
        groups => board.groups.iter().map(group_value).collect::<Vec<_>>(),
        batches => board.batches.iter().map(batch_value).collect::<Vec<_>>(),
        failures => board.failures.iter().map(failure_value).collect::<Vec<_>>(),
        overviews => overviews_value(&board.overviews),
        withheld_pulls => board.withheld,
        readiness => board.readiness.as_ref().map(readiness_value),
    }
}

#[cfg(test)]
mod tests;

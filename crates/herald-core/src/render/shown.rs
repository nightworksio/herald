//! A report's view as a template reads it: every key named here, and no
//! other.

use jiff::SignedDuration;
use minijinja::{Value, context};

use std::collections::BTreeMap;

use crate::queue::{
    Advisories, Advisory, Batch, Failure, Grouped, Overviews, Row, Running, Showing,
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

/// The overviews `overviews` holds, `capacity` absent where the view
/// shows none (ADR-0066); `advisories` present only where its surface was
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
    }
}

#[cfg(test)]
mod tests;

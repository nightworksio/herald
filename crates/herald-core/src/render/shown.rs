//! A report's view as a template reads it: every key named here, and no
//! other.

use jiff::SignedDuration;
use minijinja::{Value, context};

use crate::queue::{Batch, Grouped, Row, Showing};

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
    }
}

#[cfg(test)]
mod tests;

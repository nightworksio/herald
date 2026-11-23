//! `herald queue`: a view's open pull requests read now and printed as a
//! table or as JSON (ADR-0055).

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_compose::told::why;
use herald_config::{BUILT_IN_VIEW, Config, Uses};
use herald_core::{
    Advisories, Blocker, Board, HeldBy, Holding, Loaded, Loads, Printed, QueueEntry, Readiness,
    Row, Showing, WITHHELD, view_json, waited,
};

use super::cli::QueueFormat;
use super::output::{say, say_json};
use super::stores::composed;
use super::{Connect, DONE};

/// Where `entry` stands in its forge's merge queue, as a row's line says
/// it (ADR-0078).
fn queued(entry: &QueueEntry) -> String {
    if let Some(left) = entry.left {
        return format!(", left the merge queue: {}", left.reason.name());
    }
    let at = entry
        .position
        .map(|position| format!(" at {position}"))
        .unwrap_or_default();
    format!(", {} in the merge queue{at}", entry.state.name())
}

/// `row` on one line: the pull request, whose turn it is and why, how long
/// it waited, where it stands in the merge queue, its milestone, its areas,
/// and its title.
fn line(row: &Row) -> String {
    let stale = if row.stale { ", stale" } else { "" };
    let milestone = row
        .pull
        .milestone
        .as_ref()
        .map(|title| format!(", milestone {title}"))
        .unwrap_or_default();
    let areas = if row.pull.areas.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = row
            .pull
            .areas
            .iter()
            .map(|area| area.name.as_str())
            .collect();
        format!(", in {}", names.join(", "))
    };
    let queue = row.pull.queue.as_ref().map(queued).unwrap_or_default();
    format!(
        "  {}#{}  {}: {}, waited {}{stale}{queue}{milestone}{areas}  {}",
        row.pull.repository,
        row.pull.number,
        row.state.name(),
        row.reason.written(),
        waited(row.waited),
        row.pull.title
    )
}

/// `board` of the view `view` as a table to read.
fn table(view: &str, board: &Board) -> Vec<String> {
    let mut lines = vec![format!(
        "{view}: {} open pull requests, {} stale",
        board.total(),
        board.stale
    )];
    lines.extend(
        board
            .readiness
            .as_ref()
            .map(|readiness| judged(readiness, board.withheld))
            .into_iter()
            .flatten(),
    );
    for group in &board.groups {
        let name = if group.name.is_empty() {
            "everyone"
        } else {
            group.name.as_str()
        };
        lines.push(format!("{name} ({})", group.rows.len()));
        lines.extend(group.rows.iter().map(line));
    }
    for batch in &board.batches {
        let bump = batch.bump.as_ref().map_or_else(
            || "bumps no pattern reads".to_owned(),
            |bump| match &bump.to {
                Some(to) => format!("{} to {to}", bump.package),
                None => bump.package.clone(),
            },
        );
        lines.push(format!(
            "{bump} ({}, {} green, {} failing)",
            batch.rows.len(),
            batch.green,
            batch.failing
        ));
        lines.extend(batch.rows.iter().map(line));
    }
    lines.extend(withheld_line("", board.withheld));
    lines.extend(
        board
            .overviews
            .load
            .as_ref()
            .map(loaded)
            .into_iter()
            .flatten(),
    );
    match board
        .overviews
        .advisories
        .as_ref()
        .map(|advised| advised.shown())
    {
        Some(Some(found)) => lines.extend(advised(found)),
        Some(None) => lines.push(WITHHELD.to_owned()),
        None => {}
    }
    lines
}

/// Why `holding` holds a release back, as a line of a table says it.
fn held(holding: &Holding) -> String {
    let named = holding.named.join(", ");
    let more = if holding.more > 0 {
        format!(" and {} more", holding.more)
    } else {
        String::new()
    };
    let why = match holding.held_by {
        HeldBy::Blocked(Blocker::Label) => format!("carries {named}{more}"),
        HeldBy::Blocked(Blocker::Failed) => format!("failed {named}{more}"),
        HeldBy::Blocked(Blocker::Draft) => "is a draft".to_owned(),
        HeldBy::Blocked(Blocker::Changes) => "has changes requested".to_owned(),
        HeldBy::Unread => "was not read in full".to_owned(),
    };
    format!("  {}#{} {why}", holding.repository, holding.number)
}

/// The release verdict `readiness` holds as lines of a table: ready, or
/// not ready and each pull request holding it back, judged by the rows
/// printed where `withheld` were not (ADR-0081).
pub(crate) fn judged(readiness: &Readiness, withheld: usize) -> Vec<String> {
    if readiness.ready() {
        let printed = if withheld > 0 {
            ", by the pull requests printed"
        } else {
            ""
        };
        return vec![format!("release: ready{printed}")];
    }
    let reasons = readiness.holds.len() + readiness.more;
    let plural = if reasons == 1 { "" } else { "s" };
    let mut lines = vec![format!("release: not ready, {reasons} reason{plural}")];
    lines.extend(readiness.holds.iter().map(held));
    if readiness.more > 0 {
        lines.push(format!("  and {} more", readiness.more));
    }
    lines
}

/// The load section `loads` holds as lines of a table: each team and area
/// shown by name, then "other" (ADR-0075).
fn loaded(loads: &Loads) -> Vec<String> {
    let counted = |count: Loaded| {
        if loads.reviewed {
            format!("{} requested, {} reviewed", count.requested, count.reviewed)
        } else {
            format!("{} requested", count.requested)
        }
    };
    let mut lines = vec![if loads.reviewed {
        "load (review requests open, reviews given over the window)".to_owned()
    } else {
        "load (review requests open; reviews given are counted where every source is read kept)"
            .to_owned()
    }];
    lines.extend(
        loads
            .teams
            .iter()
            .map(|(name, count)| format!("  team {name}: {}", counted(*count))),
    );
    lines.extend(loads.areas.iter().map(|(area, count)| {
        format!(
            "  area {} ({}): {}",
            area.name,
            area.kind.name(),
            counted(*count)
        )
    }));
    lines.extend(
        loads
            .other
            .map(|count| format!("  other: {}", counted(count))),
    );
    lines
}

/// The line saying `what` of `withheld` pull requests of private or
/// internal repositories were withheld, where any were.
pub(crate) fn withheld_line(what: &str, withheld: usize) -> Option<String> {
    (withheld > 0).then(|| {
        format!(
            "{what}{withheld} pull requests of private or internal repositories withheld: --private, or [cli] private, prints them"
        )
    })
}

/// The advisories `found` holds as lines of a table: each open one, those
/// past the most read, and each listing that could not be read.
fn advised(found: &Advisories) -> Vec<String> {
    let more = if found.more { ", more not read" } else { "" };
    let mut lines = vec![format!("advisories ({}{more})", found.open.len())];
    lines.extend(found.open.iter().map(|advisory| {
        let severity = advisory
            .severity
            .as_ref()
            .map_or_else(String::new, |severity| format!(" {severity}"));
        format!(
            "  {} {}{severity}: {}, {}, {} old  {}",
            advisory.repository,
            advisory.tool,
            advisory.summary.as_deref().unwrap_or("unnamed"),
            advisory.state,
            waited(found.at.duration_since(advisory.opened)),
            advisory.url
        )
    }));
    lines.extend(
        found
            .unread
            .iter()
            .map(|unread| format!("  {} unread, because {}", unread.repository, unread.reason)),
    );
    lines
}

pub(crate) async fn queueing(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    (view, format, printed): (Option<&str>, QueueFormat, Printed),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let view = view.unwrap_or(BUILT_IN_VIEW);
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Polls,
        "queue",
    )
    .await?;
    let board = composed
        .queue(view, printed)
        .await
        .map_err(|stop| why(&stop))?;
    match format {
        QueueFormat::Json => say_json(
            out,
            &view_json(Showing {
                view,
                board: &board,
            }),
        ),
        QueueFormat::Table => {
            for line in table(view, &board) {
                say(out, &line);
            }
        }
    }
    Ok(DONE)
}

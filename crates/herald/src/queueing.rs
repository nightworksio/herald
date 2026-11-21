//! `herald queue`: a view's open pull requests read now and printed as a
//! table or as JSON (ADR-0055).

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_compose::told::why;
use herald_config::{BUILT_IN_VIEW, Config, Uses};
use herald_core::{Advisories, Board, Printed, Row, Showing, WITHHELD, view_json, waited};

use super::cli::QueueFormat;
use super::output::{say, say_json};
use super::stores::composed;
use super::{Connect, DONE};

/// `row` on one line: the pull request, whose turn it is and why, how long
/// it waited, and its title.
fn line(row: &Row) -> String {
    let stale = if row.stale { ", stale" } else { "" };
    format!(
        "  {}#{}  {}: {}, waited {}{stale}  {}",
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

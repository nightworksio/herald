//! `herald queue`: a view's open pull requests read now and printed as a
//! table or as JSON (ADR-0055).

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_compose::told::why;
use herald_config::{BUILT_IN_VIEW, Config, Uses};
use herald_core::{Board, Row, Showing, view_json, waited};

use super::cli::QueueFormat;
use super::output::say;
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
    lines
}

pub(crate) async fn queueing(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    (view, format): (Option<&str>, QueueFormat),
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
    let board = composed.queue(view).await.map_err(|stop| why(&stop))?;
    match format {
        QueueFormat::Json => say(
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

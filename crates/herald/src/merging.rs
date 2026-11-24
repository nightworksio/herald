//! `herald merge-queue`: each merge queue's entries read from its forge,
//! each with what the queue does next and why; a dry run that writes
//! nothing and reads no write credential (ADR-0087). Like `herald queue`'s
//! rows, a private or internal repository's entries are withheld and
//! counted unless the output is said private.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_compose::told::why;
use herald_config::{Config, Uses};
use herald_core::Printed;
use herald_core::merging::{Left, Verdict};
use herald_flows::{Found, Judged, MergeQueueRead, Next};

use super::output::say;
use super::queueing::withheld_line;
use super::stores::composed;
use super::{Connect, DONE};

/// The reason `left` as `queue.left` writes it.
fn reason(left: Left) -> String {
    serde_json::to_value(left)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

/// What comes next, in words.
fn next(next: &Next) -> String {
    match next {
        Next::Verdict(Verdict::Leaves(left)) => format!("leaves the queue: {}", reason(*left)),
        Next::Verdict(Verdict::Update) => {
            "next, its branch is brought up to date with the base".to_owned()
        }
        Next::Verdict(Verdict::Wait) => {
            "in flight, waiting for its checks and the forge".to_owned()
        }
        Next::Verdict(Verdict::Merge) => {
            "next, it is merged with its head as the expected head".to_owned()
        }
        Next::Behind => "waits its turn".to_owned(),
        Next::HandOff => "next, it is handed to the forge's queue".to_owned(),
        Next::Draft => "waits until it is no draft".to_owned(),
    }
}

/// `judged` on one line.
fn entry_line(judged: &Judged) -> String {
    let entered = &judged.entered;
    let role = entered
        .gesture
        .role
        .map_or("no role", herald_core::Permission::name);
    format!(
        "  #{} {} queued by {} ({role}) at {}, head {}: {}",
        entered.number,
        entered.url,
        entered.gesture.by,
        entered.gesture.at,
        entered.head,
        next(&judged.next)
    )
}

/// `read` as lines: the queue, then each entry, or why there are none; its
/// repository and branch named only beside entries the output prints.
pub(crate) fn queue_lines(read: &MergeQueueRead) -> Vec<String> {
    let mut lines = vec![match &read.found {
        Found::Entries(_) => format!(
            "{}: {} into {}, run by {}",
            read.name,
            read.repository,
            read.branch,
            read.runs.name()
        ),
        Found::Withheld(_) | Found::Unread(_) => format!("{}:", read.name),
    }];
    match &read.found {
        Found::Entries(entries) if entries.is_empty() => {
            lines.push("  no pull request is queued".to_owned());
        }
        Found::Entries(entries) => lines.extend(entries.iter().map(entry_line)),
        Found::Withheld(withheld) => lines.extend(withheld_line("  the entries of ", *withheld)),
        Found::Unread(Some(reason)) => lines.push(format!("  not read: {reason}")),
        Found::Unread(None) => {
            lines.push("  not read; --private, or [cli] private, prints why".to_owned());
        }
    }
    lines
}

pub(crate) async fn merging(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    asked: (Option<&str>, Printed),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Polls,
        "merge-queue",
    )
    .await?;
    let read = composed
        .merge_queues(asked)
        .await
        .map_err(|stop| why(&stop))?;
    if read.is_empty() {
        say(
            out,
            "no merge queue is configured; add a [merge_queues.<name>] table",
        );
    }
    for line in read.iter().flat_map(queue_lines) {
        say(out, &line);
    }
    Ok(DONE)
}

#[cfg(test)]
mod tests;

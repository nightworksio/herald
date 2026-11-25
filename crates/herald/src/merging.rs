//! `herald merge-queue`: each merge queue's entries read from its forge,
//! each with what the queue does next and why; a dry run that writes
//! nothing and reads no write credential, unless `--apply` takes the steps
//! (ADR-0087). Like `herald queue`'s rows, a private or internal
//! repository's entries are withheld and counted unless the output is said
//! private.

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_compose::told::why;
use herald_config::{Config, Uses};
use herald_core::Printed;
use herald_core::merging::{HandedOff, Left, TakenOut, Verdict};
use herald_flows::{
    Acted, Cancel, Cancels, Found, Judged, MergeQueueRead, Next, Took, TookOut, Undo,
};
use herald_native::TokioTimer;

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
        Next::Verdict(Verdict::Rerun) => {
            "next, the failed job of a check its queue's retries name is re-run".to_owned()
        }
        Next::Behind => "waits its turn".to_owned(),
        Next::HandOff => {
            "next, it is handed to the forge's queue with its head as the expected head".to_owned()
        }
        Next::Draft => "waits until it is no draft".to_owned(),
        Next::Idle => {
            "due to be handed to the forge's queue, but the queue takes no step".to_owned()
        }
        Next::Handed {
            outcome: HandedOff::FoundQueued,
            at,
        } => format!("herald recorded at {at}: {}", HandedOff::FoundQueued.said()),
        Next::Handed { outcome, at } => {
            format!(
                "herald recorded handing it to the forge's queue at {at}: {}",
                outcome.said()
            )
        }
        Next::Took(took) => took_line(took),
        Next::Held(step) => format!(
            "herald recorded {} on head {} at {}: {}; it waits",
            step.kind.said(),
            step.head,
            step.at,
            step.outcome.said()
        ),
        Next::Acted(acted) => acted_line(acted),
    }
}

/// What an apply did in herald's own queue, in words.
fn acted_line(acted: &Acted) -> String {
    match acted {
        Acted::Stepped(kind, outcome) => match outcome.left(*kind) {
            Some(left) => format!(
                "{}: {}, so it leaves the queue: {}",
                kind.said(),
                outcome.said(),
                reason(left)
            ),
            None => format!("{}: {}", kind.said(), outcome.said()),
        },
        Acted::Left(left) => format!(
            "read again right before the step, it leaves the queue: {}",
            reason(*left)
        ),
        Acted::Refused(why) => format!("no step taken: {why}"),
        Acted::Unsent(kind, why) => format!("{}: the call was not sent: {why}", kind.said()),
        Acted::Unrecorded(kind, outcome) => format!(
            "{}: {}, but herald's record of it could not be updated, so herald takes no step again for this gesture",
            kind.said(),
            outcome.said()
        ),
    }
}

/// What an apply took, in words.
fn took_line(took: &Took) -> String {
    match took {
        Took::Queued => "handed to the forge's queue, which took it".to_owned(),
        Took::Declined(said) => format!("the forge refused to take it into its queue: {said}"),
        Took::HeadMoved => {
            "not in the forge's queue: its head is no longer the one its gesture vouches for (head-moved)"
                .to_owned()
        }
        Took::Unsettled => HandedOff::Unsettled.said().to_owned(),
        Took::FoundQueued => format!("not handed over: {}", HandedOff::FoundQueued.said()),
        Took::Refused(why) => format!("not handed over: {why}"),
        Took::Unrecorded(outcome) => format!(
            "{}, but herald's record of it could not be updated, so herald never hands it over again for this gesture",
            outcome.said()
        ),
    }
}

/// What a queue does with a pull request whose gesture was taken back, or
/// whose gesture's maker lost the queue's least role, in words.
fn undo_line(undo: &Undo) -> String {
    match undo {
        Undo::TakeOut => "next, it is taken out of the forge's queue".to_owned(),
        Undo::Idle => {
            "due to be taken out of the forge's queue, but the queue takes no step".to_owned()
        }
        Undo::Uncarried(why) => format!("not taken out: {why}"),
        Undo::Stays => "role_lost = \"stay\" leaves it in the forge's queue".to_owned(),
        Undo::Recorded { outcome, at } => format!(
            "herald recorded taking it out of the forge's queue at {at}: {}",
            outcome.said()
        ),
        Undo::Took(TookOut::Out) => "taken out of the forge's queue".to_owned(),
        Undo::Took(TookOut::Declined(said)) => {
            format!("the forge refused to take it out of its queue: {said}")
        }
        Undo::Took(TookOut::Unsettled) => TakenOut::Unsettled.said().to_owned(),
        Undo::Took(TookOut::Unrecorded(outcome)) => format!(
            "{}, but herald's record of it could not be updated, so herald never asks again for this cancel",
            outcome.said()
        ),
    }
}

/// `cancel` on one line.
fn cancel_line(cancel: &Cancel) -> String {
    let taken = &cancel.taken;
    let Some(marked) = &cancel.cancel else {
        let maker = cancel.renamed.as_ref().map_or_else(
            || "whose gesture's maker no longer holds the queue's least role".to_owned(),
            |by| format!("whose gesture herald recorded as made by {by}, an account the forge no longer names for it"),
        );
        return format!(
            "  #{} {} {maker}: {}",
            taken.number,
            taken.url,
            undo_line(&cancel.next)
        );
    };
    let by = if marked.by.is_empty() {
        "an account the forge no longer names"
    } else {
        marked.by.as_str()
    };
    let role = taken
        .role_of(&marked.by)
        .map_or("no role", herald_core::Permission::name);
    format!(
        "  #{} {} taken back by {by} ({role}) at {}: {}",
        taken.number,
        taken.url,
        marked.at,
        undo_line(&cancel.next)
    )
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
        Found::Entries(entries) => {
            lines.extend(
                read.idle
                    .iter()
                    .map(|why| format!("  takes no step: {why}")),
            );
            if entries.is_empty() {
                lines.push("  no pull request is queued".to_owned());
            }
            lines.extend(entries.iter().map(entry_line));
            match &read.cancels {
                Cancels::Read(cancels) => lines.extend(cancels.iter().map(cancel_line)),
                Cancels::Unread(Some(reason)) => {
                    lines.push(format!("  the forge's queue not read for cancels: {reason}"));
                }
                Cancels::Unread(None) => lines.push(
                    "  the forge's queue not read for cancels; --private, or [cli] private, prints why"
                        .to_owned(),
                ),
            }
        }
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
    (asked, apply): ((Option<&str>, Printed), bool),
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
    let read = if apply {
        composed.merge_apply(asked, &TokioTimer).await
    } else {
        composed.merge_queues(asked).await
    }
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

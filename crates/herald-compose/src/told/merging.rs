//! What `herald merge-queue` and `herald server` say of each merge queue
//! they read and step: each entry with what comes next, each step taken
//! and what came of it (ADR-0087).

use herald_core::merging::{HandedOff, TakenOut, Verdict};
use herald_flows::{
    Acted, Cancel, Cancels, Found, Judged, MergeQueueRead, Next, Took, TookOut, Undo,
};

use crate::Told;

/// What prints what `herald server` withholds.
const SERVER_HINT: &str = "herald merge-queue --private prints";

/// What `herald server` says of `read`, where this run stepped it: the
/// queue and each entry it took a step for, each pull request it took out
/// of the forge's queue, or why the queue was not read; nothing where it
/// took no step.
#[must_use]
pub fn stepped(read: &MergeQueueRead) -> Vec<Told> {
    let entries = match &read.found {
        Found::Entries(entries) => entries,
        Found::Unread(_) => {
            return queue_lines(read, SERVER_HINT)
                .into_iter()
                .map(Told::warn)
                .collect();
        }
        Found::Withheld(_) => return Vec::new(),
    };
    let mut said: Vec<String> = entries
        .iter()
        .filter(|judged| matches!(judged.next, Next::Took(_) | Next::Acted(_)))
        .map(entry_line)
        .collect();
    if let Cancels::Read(cancels) = &read.cancels {
        said.extend(
            cancels
                .iter()
                .filter(|cancel| matches!(cancel.next, Undo::Took(_)))
                .map(cancel_line),
        );
    }
    if said.is_empty() {
        return Vec::new();
    }
    std::iter::once(heading(read))
        .chain(said)
        .map(Told::info)
        .collect()
}

/// The line naming `read`'s queue, its repository, branch and who runs it.
fn heading(read: &MergeQueueRead) -> String {
    format!(
        "{}: {} into {}, run by {}",
        read.name,
        read.repository,
        read.branch,
        read.runs.name()
    )
}

/// What comes next, in words.
fn next(next: &Next) -> String {
    match next {
        Next::Verdict(Verdict::Leaves(left)) => format!("leaves the queue: {}", left.name()),
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
                left.name()
            ),
            None => format!("{}: {}", kind.said(), outcome.said()),
        },
        Acted::Left(left) => format!(
            "read again right before the step, it leaves the queue: {}",
            left.name()
        ),
        Acted::Refused(why) => format!("no step taken: {why}"),
        Acted::Unsent(kind, why) => format!("{}: the call was not sent: {why}", kind.said()),
        Acted::Unrecorded(kind, outcome) => format!(
            "{}: {}, but herald's record of it could not be updated, so herald takes no step again for this gesture",
            kind.said(),
            outcome.said()
        ),
        Acted::Untold(left) => format!(
            "it leaves the queue: {}, recorded for its gesture, but the record a view's row reads could not be written; the next run that takes this queue's steps writes it",
            left.name()
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

/// The line saying `withheld` entries of private or internal repositories
/// were withheld, and that `hint` prints them, where any were.
fn withheld_line(withheld: usize, hint: &str) -> Option<String> {
    (withheld > 0).then(|| {
        format!(
            "  the entries of {withheld} pull requests of private or internal repositories withheld: {hint} them"
        )
    })
}

/// `read` as lines: the queue, then each entry, or why there are none; its
/// repository and branch named only beside entries the output prints, and
/// `hint` named as what prints what is withheld.
#[must_use]
pub fn queue_lines(read: &MergeQueueRead, hint: &str) -> Vec<String> {
    let mut lines = vec![match &read.found {
        Found::Entries(_) => heading(read),
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
                    lines.push(format!(
                        "  the forge's queue not read for cancels: {reason}"
                    ));
                }
                Cancels::Unread(None) => lines.push(format!(
                    "  the forge's queue not read for cancels; {hint} why"
                )),
            }
        }
        Found::Withheld(withheld) => lines.extend(withheld_line(*withheld, hint)),
        Found::Unread(Some(reason)) => lines.push(format!("  not read: {reason}")),
        Found::Unread(None) => lines.push(format!("  not read; {hint} why")),
    }
    lines
}

#[cfg(test)]
mod tests;

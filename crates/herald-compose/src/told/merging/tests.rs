use herald_core::Permission;
use herald_core::merging::{Left, Runs, Step, StepKind, Stepped};
use herald_kinds::{Entered, Gesture, Holds, Marked, TakenBack};

use super::*;

const HINT: &str = "--private, or [cli] private, prints";

fn judged(next: Next, role: Option<Permission>) -> Judged {
    Judged {
        entered: Entered {
            number: 4,
            title: String::new(),
            body: String::new(),
            url: "https://github.com/o/r/pull/4".to_owned(),
            id: None,
            draft: false,
            head: "h1".to_owned(),
            gesture: Gesture {
                id: Some(70),
                by: "w".to_owned(),
                at: "2026-01-01T00:10:00Z".parse().unwrap(),
                head: Some("h1".to_owned()),
                named: None,
                role,
            },
            arrived: None,
            approved: false,
            whole: true,
        },
        next,
    }
}

fn read(found: Found) -> MergeQueueRead {
    MergeQueueRead {
        cancels: herald_flows::Cancels::Read(Vec::new()),
        name: "app-main".to_owned(),
        repository: "o/r".to_owned(),
        branch: "main".to_owned(),
        runs: Runs::Herald,
        idle: Vec::new(),
        found,
    }
}

#[test]
fn each_entry_says_who_queued_it_and_what_comes_next() {
    let lines = queue_lines(
        &read(Found::Entries(vec![
            judged(
                Next::Verdict(Verdict::Leaves(Left::HeadMoved)),
                Some(Permission::Write),
            ),
            judged(Next::Behind, None),
        ])),
        HINT,
    );
    assert_eq!(
        lines,
        [
            "app-main: o/r into main, run by herald",
            "  #4 https://github.com/o/r/pull/4 queued by w (write) at 2026-01-01T00:10:00Z, head h1: leaves the queue: head-moved",
            "  #4 https://github.com/o/r/pull/4 queued by w (no role) at 2026-01-01T00:10:00Z, head h1: waits its turn",
        ]
    );
}

#[test]
fn every_next_step_has_its_words() {
    let said: Vec<String> = [
        Next::Verdict(Verdict::Leaves(Left::Unread)),
        Next::Verdict(Verdict::Update),
        Next::Verdict(Verdict::Wait),
        Next::Verdict(Verdict::Merge),
        Next::Verdict(Verdict::Rerun),
        Next::HandOff,
        Next::Draft,
        Next::Idle,
        Next::Handed {
            outcome: HandedOff::Calling,
            at: "2026-01-01T00:20:00Z".parse().unwrap(),
        },
        Next::Handed {
            outcome: HandedOff::Unsettled,
            at: "2026-01-01T00:20:00Z".parse().unwrap(),
        },
        Next::Handed {
            outcome: HandedOff::FoundQueued,
            at: "2026-01-01T00:20:00Z".parse().unwrap(),
        },
        Next::Took(Took::Queued),
        Next::Took(Took::FoundQueued),
        Next::Took(Took::Declined("no".to_owned())),
        Next::Took(Took::HeadMoved),
        Next::Took(Took::Unsettled),
        Next::Took(Took::Refused("why".to_owned())),
        Next::Took(Took::Unrecorded(HandedOff::Queued)),
    ]
    .iter()
    .map(next)
    .collect();
    assert_eq!(
        said,
        [
            "leaves the queue: unread",
            "next, its branch is brought up to date with the base",
            "in flight, waiting for its checks and the forge",
            "next, it is merged with its head as the expected head",
            "next, the failed job of a check its queue's retries name is re-run",
            "next, it is handed to the forge's queue with its head as the expected head",
            "waits until it is no draft",
            "due to be handed to the forge's queue, but the queue takes no step",
            "herald recorded handing it to the forge's queue at 2026-01-01T00:20:00Z: the call may or may not have reached the forge, so herald never hands it over again for this gesture",
            "herald recorded handing it to the forge's queue at 2026-01-01T00:20:00Z: the forge's answer was lost and it was not found in the forge's queue, so herald never hands it over again for this gesture",
            "herald recorded at 2026-01-01T00:20:00Z: it was already in the forge's queue, so herald made no call; a cancel or a lost role takes it out as if herald had handed it over",
            "handed to the forge's queue, which took it",
            "not handed over: it was already in the forge's queue, so herald made no call; a cancel or a lost role takes it out as if herald had handed it over",
            "the forge refused to take it into its queue: no",
            "not in the forge's queue: its head is no longer the one its gesture vouches for (head-moved)",
            "the forge's answer was lost and it was not found in the forge's queue, so herald never hands it over again for this gesture",
            "not handed over: why",
            "the forge took it into its queue, but herald's record of it could not be updated, so herald never hands it over again for this gesture",
        ]
    );
}

#[test]
fn every_step_of_herald_s_own_queue_has_its_words() {
    let step = Step {
        kind: StepKind::Update,
        head: "h1".to_owned(),
        base: "b1".to_owned(),
        clean: true,
        made: None,
        check: None,
        credential: "HERALD_MERGE".to_owned(),
        at: "2026-01-01T00:20:00Z".parse().unwrap(),
        outcome: Stepped::Calling,
    };
    let said: Vec<String> = [
        Next::Held(step),
        Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took)),
        Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Conflict)),
        Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Refused)),
        Next::Acted(Acted::Stepped(StepKind::Rerun, Stepped::Refused)),
        Next::Acted(Acted::Left(Left::HeadMoved)),
        Next::Acted(Acted::Refused("why".to_owned())),
        Next::Acted(Acted::Unsent(StepKind::Merge, "why".to_owned())),
        Next::Acted(Acted::Unrecorded(StepKind::Update, Stepped::Took)),
        Next::Acted(Acted::Untold(Left::Unread)),
    ]
    .iter()
    .map(next)
    .collect();
    assert_eq!(
        said,
        [
            "herald recorded bringing it up to date on head h1 at 2026-01-01T00:20:00Z: the call may or may not have reached the forge, so herald never takes a step again for this gesture; it waits",
            "merging it: the forge took it",
            "bringing it up to date: the branch conflicts with the base, so it leaves the queue: conflict",
            "merging it: the forge refused it, so it leaves the queue: merge-refused",
            "re-running a failed check: the forge refused it, so it leaves the queue: checks-failed",
            "read again right before the step, it leaves the queue: head-moved",
            "no step taken: why",
            "merging it: the call was not sent: why",
            "bringing it up to date: the forge took it, but herald's record of it could not be updated, so herald takes no step again for this gesture",
            "it leaves the queue: unread, recorded for its gesture, but the record a view's row reads could not be written; the next run that takes this queue's steps writes it",
        ]
    );
}

#[test]
fn a_queue_with_nothing_to_show_says_why() {
    let empty = queue_lines(&read(Found::Entries(Vec::new())), HINT);
    assert_eq!(empty[1], "  no pull request is queued");
    let unread = queue_lines(
        &read(Found::Unread(Some("the forge refused".to_owned()))),
        HINT,
    );
    assert_eq!(unread[1], "  not read: the forge refused");
    let unsaid = queue_lines(&read(Found::Unread(None)), HINT);
    assert_eq!(
        unsaid[1],
        "  not read; --private, or [cli] private, prints why"
    );
    assert_eq!(unsaid[0], "app-main:");
    let withheld = queue_lines(&read(Found::Withheld(2)), HINT);
    assert_eq!(withheld[0], "app-main:");
    assert_eq!(withheld.len(), 2);
    assert!(withheld[1].starts_with("  the entries of 2 pull requests"));
    assert_eq!(queue_lines(&read(Found::Withheld(0)), HINT).len(), 1);
}

fn cancel(next: Undo, (by, role): (&str, Option<Permission>)) -> Cancel {
    let marked = Marked {
        id: Some(12),
        by: by.to_owned(),
        at: "2026-01-01T00:12:00Z".parse().unwrap(),
    };
    let makers = role.map(|role| (by.to_owned(), Holds::Role(role)));
    Cancel {
        taken: TakenBack {
            number: 5,
            url: "https://github.com/o/r/pull/5".to_owned(),
            id: None,
            gestures: Vec::new(),
            cancels: vec![marked.clone()],
            makers: makers.into_iter().collect(),
            whole: true,
        },
        cancel: Some(marked),
        next,
        renamed: None,
    }
}

#[test]
fn a_pull_request_whose_gesture_s_maker_lost_the_role_says_so_or_that_it_stays() {
    let mut lost = cancel(Undo::Took(TookOut::Out), ("", None));
    lost.cancel = None;
    assert_eq!(
        cancel_line(&lost),
        "  #5 https://github.com/o/r/pull/5 whose gesture's maker no longer holds the queue's least role: taken out of the forge's queue"
    );
    lost.renamed = Some("w".to_owned());
    lost.next = Undo::Stays;
    assert_eq!(
        cancel_line(&lost),
        "  #5 https://github.com/o/r/pull/5 whose gesture herald recorded as made by w, an account the forge no longer names for it: role_lost = \"stay\" leaves it in the forge's queue"
    );
}

#[test]
fn each_cancel_says_who_took_it_back_and_what_comes_of_it() {
    let mut cancelled = read(Found::Entries(Vec::new()));
    cancelled.cancels = herald_flows::Cancels::Read(vec![
        cancel(Undo::TakeOut, ("x", Some(Permission::Maintain))),
        cancel(Undo::Uncarried("why".to_owned()), ("", None)),
    ]);
    assert_eq!(
        queue_lines(&cancelled, HINT)[2..],
        [
            "  #5 https://github.com/o/r/pull/5 taken back by x (maintain) at 2026-01-01T00:12:00Z: next, it is taken out of the forge's queue",
            "  #5 https://github.com/o/r/pull/5 taken back by an account the forge no longer names (no role) at 2026-01-01T00:12:00Z: not taken out: why",
        ]
    );
    cancelled.cancels = herald_flows::Cancels::Unread(Some("the forge refused".to_owned()));
    assert_eq!(
        queue_lines(&cancelled, HINT)[2],
        "  the forge's queue not read for cancels: the forge refused"
    );
    cancelled.cancels = herald_flows::Cancels::Unread(None);
    assert_eq!(
        queue_lines(&cancelled, HINT)[2],
        "  the forge's queue not read for cancels; --private, or [cli] private, prints why"
    );
}

#[test]
fn every_undoing_has_its_words() {
    let at = "2026-01-01T00:20:00Z".parse().unwrap();
    let said: Vec<String> = [
        Undo::Idle,
        Undo::Uncarried("why".to_owned()),
        Undo::Recorded {
            outcome: TakenOut::Out,
            at,
        },
        Undo::Took(TookOut::Out),
        Undo::Took(TookOut::Declined("no".to_owned())),
        Undo::Took(TookOut::Unsettled),
        Undo::Took(TookOut::Unrecorded(TakenOut::Out)),
    ]
    .iter()
    .map(undo_line)
    .collect();
    assert_eq!(
        said,
        [
            "due to be taken out of the forge's queue, but the queue takes no step".to_owned(),
            "not taken out: why".to_owned(),
            format!(
                "herald recorded taking it out of the forge's queue at 2026-01-01T00:20:00Z: {}",
                TakenOut::Out.said()
            ),
            "taken out of the forge's queue".to_owned(),
            "the forge refused to take it out of its queue: no".to_owned(),
            TakenOut::Unsettled.said().to_owned(),
            format!(
                "{}, but herald's record of it could not be updated, so herald never asks again for this cancel",
                TakenOut::Out.said()
            ),
        ]
    );
}

#[test]
fn the_server_says_each_step_it_took_and_nothing_of_a_queue_it_took_none_in() {
    let idle = read(Found::Entries(vec![
        judged(Next::Behind, None),
        judged(Next::Verdict(Verdict::Wait), None),
    ]));
    assert_eq!(stepped(&idle), Vec::<String>::new());
    assert_eq!(stepped(&read(Found::Withheld(3))), Vec::<String>::new());
    assert_eq!(
        stepped(&read(Found::Unread(None))),
        [
            "app-main:",
            "  not read; herald merge-queue --private prints why"
        ]
    );
    let mut taken = read(Found::Entries(vec![
        judged(Next::Behind, None),
        judged(Next::Took(Took::Queued), Some(Permission::Write)),
        judged(
            Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took)),
            Some(Permission::Write),
        ),
    ]));
    taken.cancels = Cancels::Read(vec![
        cancel(Undo::TakeOut, ("x", Some(Permission::Write))),
        cancel(Undo::Took(TookOut::Out), ("x", Some(Permission::Write))),
    ]);
    let mut unread = taken.clone();
    unread.cancels = Cancels::Unread(None);
    assert_eq!(stepped(&unread).len(), 3);
    let said = stepped(&taken);
    assert_eq!(said.len(), 4, "{said:?}");
    assert_eq!(said[0], "app-main: o/r into main, run by herald");
    assert!(
        said[1]
            .text
            .ends_with("handed to the forge's queue, which took it")
    );
    assert!(said[2].text.ends_with("merging it: the forge took it"));
    assert!(said[3].text.ends_with("taken out of the forge's queue"));
}

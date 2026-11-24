use herald_core::Permission;
use herald_core::merging::Runs;
use herald_kinds::{Entered, Gesture};

use super::*;

fn judged(next: Next, role: Option<Permission>) -> Judged {
    Judged {
        entered: Entered {
            number: 4,
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
    let lines = queue_lines(&read(Found::Entries(vec![
        judged(
            Next::Verdict(Verdict::Leaves(Left::HeadMoved)),
            Some(Permission::Write),
        ),
        judged(Next::Behind, None),
    ])));
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
        Next::Took(Took::Queued),
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
            "next, it is handed to the forge's queue with its head as the expected head",
            "waits until it is no draft",
            "due to be handed to the forge's queue, but the queue takes no step",
            "herald recorded handing it to the forge's queue at 2026-01-01T00:20:00Z: the call may or may not have reached the forge, so herald never hands it over again for this gesture",
            "herald recorded handing it to the forge's queue at 2026-01-01T00:20:00Z: the forge's answer was lost and it was not found in the forge's queue, so herald never hands it over again for this gesture",
            "handed to the forge's queue, which took it",
            "the forge refused to take it into its queue: no",
            "not in the forge's queue: its head is no longer the one its gesture vouches for (head-moved)",
            "the forge's answer was lost and it was not found in the forge's queue, so herald never hands it over again for this gesture",
            "not handed over: why",
            "the forge took it into its queue, but herald's record of it could not be updated, so herald never hands it over again for this gesture",
        ]
    );
}

#[test]
fn a_queue_with_nothing_to_show_says_why() {
    let empty = queue_lines(&read(Found::Entries(Vec::new())));
    assert_eq!(empty[1], "  no pull request is queued");
    let unread = queue_lines(&read(Found::Unread(Some("the forge refused".to_owned()))));
    assert_eq!(unread[1], "  not read: the forge refused");
    let unsaid = queue_lines(&read(Found::Unread(None)));
    assert_eq!(
        unsaid[1],
        "  not read; --private, or [cli] private, prints why"
    );
    assert_eq!(unsaid[0], "app-main:");
    let withheld = queue_lines(&read(Found::Withheld(2)));
    assert_eq!(withheld[0], "app-main:");
    assert_eq!(withheld.len(), 2);
    assert!(withheld[1].starts_with("  the entries of 2 pull requests"));
    assert_eq!(queue_lines(&read(Found::Withheld(0))).len(), 1);
}

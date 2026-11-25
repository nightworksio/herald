use super::*;

const HEAD: &str = "abc123";

fn check(name: &str, state: CheckState) -> Check {
    Check {
        name: name.to_owned(),
        state,
    }
}

fn green<'a>(checks: &'a [Check], required: &'a [String]) -> Read<'a> {
    Read {
        entered: true,
        vouches: Vouches::Gesture,
        heads: Heads {
            then: Some(HEAD),
            ..Heads::default()
        },
        head: HEAD,
        up_to_date: true,
        clean: Some(true),
        checks,
        required,
        mergeable: Some(true),
        waited: SignedDuration::from_mins(5),
        whole: true,
    }
}

const WAIT: SignedDuration = SignedDuration::from_hours(2);

const OTHER: &str = "def456";

fn heads(then: Option<&'static str>, seen: Option<&'static str>, approved: bool) -> Heads<'static> {
    Heads {
        then,
        named: None,
        seen,
        approved,
    }
}

const SETTINGS: [Vouches; 3] = [Vouches::Gesture, Vouches::Seen, Vouches::Approved];

#[test]
fn every_cell_of_the_vouching_table_holds() {
    for vouches in SETTINGS {
        for named in [false, true] {
            for approved in [false, true] {
                for moved in [false, true] {
                    let recorded = Some(HEAD);
                    let heads = Heads {
                        then: if named { Some(OTHER) } else { recorded },
                        named: named.then_some(HEAD),
                        seen: if named { None } else { recorded },
                        approved,
                    };
                    let head = if moved { OTHER } else { HEAD };
                    let expected = !moved && (vouches != Vouches::Approved || approved);
                    assert_eq!(
                        vouched(vouches, &heads, head),
                        expected,
                        "{vouches:?} named {named} approved {approved} moved {moved}"
                    );
                }
            }
        }
    }
}

#[test]
fn each_setting_reads_its_own_record_of_the_head() {
    let only = |then, seen| heads(then, seen, true);
    assert!(vouched(Vouches::Gesture, &only(Some(HEAD), None), HEAD));
    assert!(!vouched(Vouches::Gesture, &only(None, Some(HEAD)), HEAD));
    for first_seen in [Vouches::Seen, Vouches::Approved] {
        assert!(vouched(first_seen, &only(None, Some(HEAD)), HEAD));
        assert!(!vouched(first_seen, &only(Some(HEAD), None), HEAD));
    }
}

#[test]
fn a_head_no_record_gives_leaves_unrecorded_where_the_first_seen_head_is_asked() {
    let none = heads(None, None, true);
    assert_eq!(
        unvouched(Vouches::Gesture, &none, HEAD),
        Some(Left::HeadMoved)
    );
    assert_eq!(
        unvouched(Vouches::Seen, &none, HEAD),
        Some(Left::Unrecorded)
    );
    assert_eq!(
        unvouched(Vouches::Approved, &none, HEAD),
        Some(Left::Unrecorded)
    );
    let named = Heads {
        named: Some(OTHER),
        ..none
    };
    assert_eq!(
        unvouched(Vouches::Seen, &named, HEAD),
        Some(Left::HeadMoved)
    );
    let moved = heads(None, Some(OTHER), true);
    assert_eq!(
        unvouched(Vouches::Approved, &moved, HEAD),
        Some(Left::HeadMoved)
    );
    let unapproved = heads(None, Some(HEAD), false);
    assert_eq!(
        unvouched(Vouches::Approved, &unapproved, HEAD),
        Some(Left::HeadMoved)
    );
    assert_eq!(unvouched(Vouches::Seen, &unapproved, HEAD), None);
    let read = Read {
        vouches: Vouches::Seen,
        heads: none,
        ..green(&[], &[])
    };
    assert_eq!(verdict(&read, &[], WAIT), Verdict::Leaves(Left::Unrecorded));
}

#[test]
fn a_pull_request_up_to_date_with_every_check_passed_and_mergeable_is_merged() {
    let checks = [check("test", CheckState::Passed)];
    let required = ["test".to_owned()];
    assert_eq!(
        verdict(&green(&checks, &required), &[], WAIT),
        Verdict::Merge
    );
}

#[test]
fn a_taken_back_gesture_or_a_moved_head_leaves_before_anything_else() {
    let checks = [check("test", CheckState::Failed)];
    let removed = Read {
        entered: false,
        head: "def456",
        ..green(&checks, &[])
    };
    assert_eq!(verdict(&removed, &[], WAIT), Verdict::Leaves(Left::Removed));
    let moved = Read {
        head: "def456",
        up_to_date: false,
        ..green(&checks, &[])
    };
    assert_eq!(verdict(&moved, &[], WAIT), Verdict::Leaves(Left::HeadMoved));
}

#[test]
fn a_read_not_whole_leaves_unread_before_its_head_or_checks_are_weighed() {
    let checks = [check("test", CheckState::Passed)];
    let partial = Read {
        whole: false,
        head: "def456",
        ..green(&checks, &[])
    };
    assert_eq!(verdict(&partial, &[], WAIT), Verdict::Leaves(Left::Unread));
    let taken = Read {
        entered: false,
        ..partial
    };
    assert_eq!(verdict(&taken, &[], WAIT), Verdict::Leaves(Left::Removed));
}

#[test]
fn a_branch_behind_its_base_is_brought_up_to_date_before_its_checks_count() {
    let checks = [check("test", CheckState::Failed)];
    let behind = Read {
        up_to_date: false,
        ..green(&checks, &[])
    };
    assert_eq!(verdict(&behind, &[], WAIT), Verdict::Update);
    let conflicting = Read {
        clean: Some(false),
        ..behind
    };
    assert_eq!(
        verdict(&conflicting, &[], WAIT),
        Verdict::Leaves(Left::Conflict)
    );
    let unknown = Read {
        clean: None,
        ..behind
    };
    assert_eq!(verdict(&unknown, &[], WAIT), Verdict::Wait);
    let outlasted = Read {
        waited: WAIT,
        ..unknown
    };
    assert_eq!(
        verdict(&outlasted, &[], WAIT),
        Verdict::Leaves(Left::ChecksTimedOut)
    );
    let passed = [check("test", CheckState::Passed)];
    let up_to_date = Read {
        up_to_date: true,
        clean: None,
        ..green(&passed, &[])
    };
    assert_eq!(verdict(&up_to_date, &[], WAIT), Verdict::Merge);
}

#[test]
fn a_failed_check_leaves_unless_may_fail_names_it_and_the_forge_does_not_require_it() {
    let checks = [
        check("test", CheckState::Passed),
        check("flaky", CheckState::Failed),
    ];
    let flaky = ["flaky".to_owned()];
    assert_eq!(
        verdict(&green(&checks, &[]), &[], WAIT),
        Verdict::Leaves(Left::ChecksFailed)
    );
    assert_eq!(verdict(&green(&checks, &[]), &flaky, WAIT), Verdict::Merge);
    assert_eq!(
        verdict(&green(&checks, &flaky), &flaky, WAIT),
        Verdict::Leaves(Left::ChecksFailed)
    );
    assert_eq!(failing(&green(&checks, &[]), &[]), ["flaky"]);
    assert_eq!(failing(&green(&checks, &[]), &flaky), Vec::<&str>::new());
    assert_eq!(failing(&green(&checks, &flaky), &flaky), ["flaky"]);
}

#[test]
fn a_pending_or_missing_check_or_an_unanswered_forge_is_waited_for_until_wait_has_passed() {
    let pending = [check("test", CheckState::Pending)];
    let passed = [check("test", CheckState::Passed)];
    let required = ["build".to_owned()];
    let flaky = ["test".to_owned()];
    for read in [
        green(&pending, &[]),
        green(&[], &required),
        green(&[], &[]),
        Read {
            mergeable: None,
            ..green(&passed, &[])
        },
        Read {
            mergeable: None,
            ..green(&[], &[])
        },
    ] {
        assert_eq!(verdict(&read, &[], WAIT), Verdict::Wait);
        let late = Read {
            waited: WAIT,
            ..read
        };
        assert_eq!(
            verdict(&late, &[], WAIT),
            Verdict::Leaves(Left::ChecksTimedOut)
        );
    }
    assert_eq!(verdict(&green(&pending, &[]), &flaky, WAIT), Verdict::Wait);
    assert_eq!(verdict(&green(&passed, &[]), &flaky, WAIT), Verdict::Wait);
    let failed_flaky = [check("test", CheckState::Failed)];
    assert_eq!(
        verdict(
            &Read {
                waited: WAIT,
                ..green(&failed_flaky, &[])
            },
            &flaky,
            WAIT
        ),
        Verdict::Leaves(Left::ChecksTimedOut)
    );
    let counted = [
        check("test", CheckState::Failed),
        check("build", CheckState::Passed),
    ];
    assert_eq!(verdict(&green(&counted, &[]), &flaky, WAIT), Verdict::Merge);
    let missing = ["test".to_owned()];
    assert_eq!(verdict(&green(&[], &missing), &flaky, WAIT), Verdict::Wait);
}

#[test]
fn a_pull_request_the_forge_says_cannot_merge_leaves() {
    let passed = [check("test", CheckState::Passed)];
    let unmergeable = Read {
        mergeable: Some(false),
        ..green(&passed, &[])
    };
    assert_eq!(
        verdict(&unmergeable, &[], WAIT),
        Verdict::Leaves(Left::Unmergeable)
    );
}

#[test]
fn every_reason_to_leave_but_a_taken_back_gesture_is_a_failure() {
    for (left, failed, written) in [
        (Left::ChecksFailed, true, "\"checks-failed\""),
        (Left::ChecksTimedOut, true, "\"checks-timed-out\""),
        (Left::Conflict, true, "\"conflict\""),
        (Left::CannotUpdate, true, "\"cannot-update\""),
        (Left::HeadMoved, true, "\"head-moved\""),
        (Left::Removed, false, "\"removed\""),
        (Left::Unmergeable, true, "\"unmergeable\""),
        (Left::MergeRefused, true, "\"merge-refused\""),
        (Left::Unread, true, "\"unread\""),
        (Left::Unrecorded, true, "\"unrecorded\""),
    ] {
        assert_eq!(left.failed(), failed, "{left:?}");
        assert_eq!(serde_json::to_string(&left).unwrap(), written);
    }
}

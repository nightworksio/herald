use super::*;

pub(crate) fn pull() -> Pull {
    Pull {
        project: "herald".to_owned(),
        repository: "nightworksio/herald".to_owned(),
        visibility: Visibility::Public,
        number: 7,
        title: "Add a queue".to_owned(),
        url: "https://github.com/nightworksio/herald/pull/7".to_owned(),
        author: Some("wessel".to_owned()),
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: Timestamp::from_second(1_000).unwrap(),
        updated: Timestamp::from_second(2_000).unwrap(),
        review: Review::Approved,
        reviewers: Vec::new(),
        checks: Checks::Passed,
        mergeable: Mergeable::Clean,
        queue: None,
        forge: None,
        head: Some("abc123".to_owned()),
        unread: false,
        base: None,
        files: None,
        areas: Vec::new(),
        head_branch: None,
        milestone: None,
        flips: Vec::new(),
    }
}

fn failed() -> Checks {
    Checks::Failed(vec!["test".to_owned(), "lint".to_owned()])
}

#[test]
fn every_turn_has_a_name_it_is_found_by() {
    let names: Vec<&str> = Turn::ALL.iter().map(|turn| turn.name()).collect();
    assert_eq!(
        names,
        [
            "waiting-on-review",
            "waiting-on-merge",
            "waiting-on-author",
            "waiting-on-checks",
            "queued",
            "draft",
            "unknown",
        ]
    );
    for turn in Turn::ALL {
        assert_eq!(Turn::named(turn.name()), Some(turn));
    }
    assert_eq!(Turn::named("waiting"), None);
}

#[test]
fn an_approved_green_mergeable_pull_waits_on_merge() {
    assert_eq!(
        pull().waiting(&Rules::default()),
        (Turn::WaitingOnMerge, Reason::Ready)
    );
}

#[test]
fn the_first_rule_that_holds_tells_whose_turn_it_is() {
    let worst = Pull {
        draft: true,
        queue: Some(QueueEntry::entered(None)),
        mergeable: Mergeable::Conflict,
        review: Review::ChangesRequested,
        checks: failed(),
        ..pull()
    };
    let mut rules = Rules::default();
    assert_eq!(worst.waiting(&rules), (Turn::Draft, Reason::Draft));
    rules.draft = false;
    assert_eq!(worst.waiting(&rules), (Turn::Queued, Reason::Queued));
    rules.queued = false;
    assert_eq!(
        worst.waiting(&rules),
        (Turn::WaitingOnAuthor, Reason::Conflict)
    );
    rules.conflict = false;
    assert_eq!(
        worst.waiting(&rules),
        (Turn::WaitingOnAuthor, Reason::ChangesRequested)
    );
    rules.changes = false;
    assert_eq!(
        worst.waiting(&rules),
        (
            Turn::WaitingOnAuthor,
            Reason::CheckFailed(vec!["test".to_owned(), "lint".to_owned()])
        )
    );
    rules.failed = false;
    assert_eq!(
        worst.waiting(&rules),
        (Turn::WaitingOnReview, Reason::Unapproved)
    );
    rules.approval = false;
    assert_eq!(worst.waiting(&rules), (Turn::WaitingOnMerge, Reason::Ready));
}

#[test]
fn one_listed_past_the_most_read_in_full_is_unknown_unless_a_draft() {
    let unread = Pull {
        unread: true,
        ..pull()
    };
    assert_eq!(
        unread.waiting(&Rules::default()),
        (Turn::Unknown, Reason::Unread)
    );
    let drafted = Pull {
        draft: true,
        ..unread
    };
    assert_eq!(
        drafted.waiting(&Rules::default()),
        (Turn::Draft, Reason::Draft)
    );
}

#[test]
fn a_review_herald_cannot_tell_decides_leaves_the_turn_unknown() {
    let unknown = Pull {
        review: Review::Unknown,
        ..pull()
    };
    assert_eq!(
        unknown.waiting(&Rules::default()),
        (Turn::Unknown, Reason::ReviewUnknown)
    );
    let rules = Rules {
        approval: false,
        ..Rules::default()
    };
    assert_eq!(
        unknown.waiting(&rules),
        (Turn::WaitingOnMerge, Reason::Ready)
    );
}

#[test]
fn running_checks_wait_on_the_checks_until_that_rule_is_off() {
    let running = Pull {
        checks: Checks::Pending,
        ..pull()
    };
    assert_eq!(
        running.waiting(&Rules::default()),
        (Turn::WaitingOnChecks, Reason::ChecksPending)
    );
    let rules = Rules {
        pending: false,
        ..Rules::default()
    };
    assert_eq!(
        running.waiting(&rules),
        (Turn::WaitingOnMerge, Reason::Ready)
    );
}

#[test]
fn an_unapproved_pull_waits_on_review_saying_whether_one_was_asked() {
    let asked = Pull {
        review: Review::Requested,
        ..pull()
    };
    let unasked = Pull {
        review: Review::Unasked,
        ..pull()
    };
    let rules = Rules::default();
    assert_eq!(
        asked.waiting(&rules),
        (Turn::WaitingOnReview, Reason::ReviewRequested)
    );
    assert_eq!(
        unasked.waiting(&rules),
        (Turn::WaitingOnReview, Reason::Unapproved)
    );
    let unrequired = Rules {
        approval: false,
        ..rules
    };
    assert_eq!(
        unasked.waiting(&unrequired),
        (Turn::WaitingOnMerge, Reason::Ready)
    );
}

#[test]
fn each_rule_switched_off_reads_only_its_own_condition() {
    let rules = Rules {
        draft: false,
        queued: false,
        conflict: false,
        changes: false,
        failed: false,
        pending: false,
        approval: false,
    };
    let everything = Pull {
        draft: true,
        queue: Some(QueueEntry::entered(None)),
        mergeable: Mergeable::Conflict,
        review: Review::ChangesRequested,
        checks: Checks::Pending,
        ..pull()
    };
    assert_eq!(
        everything.waiting(&rules),
        (Turn::WaitingOnMerge, Reason::Ready)
    );
    let unknown = Pull {
        mergeable: Mergeable::Unknown,
        ..pull()
    };
    assert_eq!(
        unknown.waiting(&Rules::default()),
        (Turn::WaitingOnMerge, Reason::Ready)
    );
}

#[test]
fn the_forge_s_own_status_tells_where_it_gives_one() {
    assert_eq!(pull().forge_waiting(), (Turn::Unknown, Reason::Untold));
    let mapped = Pull {
        forge: Some((Turn::WaitingOnChecks, "ci_still_running".to_owned())),
        ..pull()
    };
    assert_eq!(
        mapped.forge_waiting(),
        (
            Turn::WaitingOnChecks,
            Reason::Forge("ci_still_running".to_owned())
        )
    );
}

#[test]
fn the_first_label_listed_that_a_pull_carries_tells_ignoring_case() {
    let labels = vec![
        ("awaiting-review".to_owned(), Turn::WaitingOnReview),
        ("needs-work".to_owned(), Turn::WaitingOnAuthor),
    ];
    let labelled = Pull {
        labels: vec!["Needs-Work".to_owned(), "AWAITING-REVIEW".to_owned()],
        ..pull()
    };
    assert_eq!(
        labelled.labelled_waiting(&labels),
        (
            Turn::WaitingOnReview,
            Reason::Label("awaiting-review".to_owned())
        )
    );
    let other = Pull {
        labels: vec!["needs-work".to_owned()],
        ..pull()
    };
    assert_eq!(
        other.labelled_waiting(&labels),
        (
            Turn::WaitingOnAuthor,
            Reason::Label("needs-work".to_owned())
        )
    );
    assert_eq!(
        pull().labelled_waiting(&labels),
        (Turn::Unknown, Reason::Untold)
    );
}

#[test]
fn green_needs_passed_checks_and_a_clean_merge() {
    assert!(pull().green());
    for checks in [Checks::Absent, Checks::Pending, failed()] {
        assert!(!Pull { checks, ..pull() }.green());
    }
    for mergeable in [Mergeable::Unknown, Mergeable::Conflict] {
        assert!(
            !Pull {
                mergeable,
                ..pull()
            }
            .green()
        );
    }
    assert!(
        Pull {
            checks: failed(),
            ..pull()
        }
        .failing()
    );
    assert!(
        !Pull {
            checks: Checks::Pending,
            ..pull()
        }
        .failing()
    );
    assert!(!pull().failing());
}

#[test]
fn every_reason_is_written_as_a_row_names_it() {
    let written: Vec<String> = [
        Reason::Draft,
        Reason::Queued,
        Reason::Conflict,
        Reason::ChangesRequested,
        Reason::CheckFailed(vec!["test".to_owned(), "lint".to_owned()]),
        Reason::ChecksPending,
        Reason::ReviewRequested,
        Reason::Unapproved,
        Reason::ReviewUnknown,
        Reason::Ready,
        Reason::Forge("blocked".to_owned()),
        Reason::Label("needs-work".to_owned()),
        Reason::Unread,
        Reason::Untold,
    ]
    .iter()
    .map(Reason::written)
    .collect();
    assert_eq!(
        written,
        [
            "draft",
            "queued",
            "conflict",
            "changes-requested",
            "check-failed: test, lint",
            "checks-pending",
            "review-requested",
            "unapproved",
            "review-unknown",
            "ready",
            "forge: blocked",
            "label: needs-work",
            "unread",
            "untold",
        ]
    );
}

#[test]
fn turns_serialise_by_their_names() {
    for turn in Turn::ALL {
        assert_eq!(
            serde_json::to_value(turn).unwrap(),
            serde_json::Value::String(turn.name().to_owned())
        );
    }
}

#[test]
fn a_pull_request_that_left_the_queue_no_longer_waits_on_it() {
    let left = Left {
        at: Timestamp::from_second(3_000).unwrap(),
        reason: Dequeued::ChecksFailed,
    };
    let gone = Pull {
        queue: Some(QueueEntry::leaving(None, left)),
        ..pull()
    };
    assert!(!gone.queued());
    assert_eq!(
        gone.waiting(&Rules::default()),
        (Turn::WaitingOnMerge, Reason::Ready)
    );
    let waiting = Pull {
        queue: Some(QueueEntry::entered(None)),
        ..pull()
    };
    assert!(waiting.queued() && !pull().queued());
}

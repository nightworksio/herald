use super::*;
use crate::queue::tests::pull;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

fn key() -> PullKey {
    PullKey {
        repository: RepositoryKey::of("https://github.com/nightworksio/herald/pull/7", "42")
            .unwrap(),
        number: 7,
    }
}

fn described(updated: i64) -> Described {
    Described {
        number: 7,
        project: "herald".to_owned(),
        repository: "nightworksio/herald".to_owned(),
        visibility: None,
        title: "Add a queue".to_owned(),
        url: "https://github.com/nightworksio/herald/pull/7".to_owned(),
        author: Some(("wessel".to_owned(), false)),
        draft: false,
        labels: Some(vec!["queue".to_owned()]),
        reviewers: Some(vec!["octo".to_owned()]),
        head: Some("abc123".to_owned()),
        mergeable: None,
        queued: None,
        forge: None,
        opened: at(1_000),
        updated: at(updated),
        lifecycle: Lifecycle::Open,
    }
}

fn kept() -> KeptPull {
    KeptPull::read(None, key(), pull(), at(3_000))
}

fn review(reviewer: &str, verdict: Option<Verdict>, decides: Option<bool>, second: i64) -> Folded {
    Folded::Review {
        number: 7,
        reviewer: reviewer.to_owned(),
        verdict,
        decides,
        at: at(second),
    }
}

fn check(head: &str, name: &str, outcome: Outcome, second: i64) -> Folded {
    Folded::Check {
        head: head.to_owned(),
        name: name.to_owned(),
        outcome,
        at: at(second),
    }
}

fn folded(kept: KeptPull, folds: &[Folded]) -> KeptPull {
    folds.iter().fold(kept, |kept, folded| {
        KeptPull::fold(Some(kept), key(), folded).unwrap()
    })
}

#[test]
fn a_pull_request_is_kept_under_the_host_of_its_page_its_repository_s_id_and_its_number() {
    assert_eq!(key().to_string(), "github.com/42/7");
    assert_eq!(
        RepositoryKey::of("https://git.example.com:8443/o/r/pulls/1", "9")
            .unwrap()
            .to_string(),
        "git.example.com:8443/9"
    );
    assert_eq!(RepositoryKey::of("not a page", "9"), None);
}

#[test]
fn a_read_keeps_what_it_read_and_moves_only_the_parts_that_changed() {
    let first = kept();
    assert_eq!(first.since, Since::at(at(3_000)));
    assert_eq!(
        first.read,
        Some(Read {
            at: at(3_000),
            head: Some("abc123".to_owned()),
            review: Review::Approved,
            checks: Checks::Passed,
        })
    );
    assert!(first.open());
    let mut changed = pull();
    changed.draft = true;
    changed.checks = Checks::Pending;
    let folded = folded(first, &[check("abc123", "test", Outcome::Failed, 3_100)]);
    let again = KeptPull::read(Some(&folded.vanished(at(3_200))), key(), changed, at(4_000));
    assert_eq!(
        again.since,
        Since {
            draft: at(4_000),
            checks: at(4_000),
            ..Since::at(at(3_000))
        }
    );
    assert!(again.checks.is_empty() && again.reviews.is_empty() && again.open());
    let mut unread = pull();
    unread.unread = true;
    assert_eq!(KeptPull::read(None, key(), unread, at(1)).read, None);
}

#[test]
fn a_listing_reads_again_only_what_changed_was_never_read_closed_or_still_runs() {
    let kept = kept();
    assert!(!kept.stale(&pull()));
    let mut moved = pull();
    moved.head = Some("def456".to_owned());
    assert!(kept.stale(&moved));
    let mut touched = pull();
    touched.updated = at(2_001);
    assert!(kept.stale(&touched));
    assert!(kept.clone().vanished(at(4_000)).stale(&pull()));
    let mut running = kept.clone();
    running.pull.checks = Checks::Pending;
    assert!(running.stale(&pull()));
    let mut unread = kept.clone();
    unread.read = None;
    assert!(unread.stale(&pull()));
    let mut renamed = pull();
    renamed.repository = "nightworksio/beacon".to_owned();
    assert!(kept.stale(&renamed));
    let mut hidden = pull();
    hidden.visibility = Visibility::Private;
    assert!(!kept.stale(&hidden));
    let mut searched = pull();
    searched.head = None;
    assert!(!kept.stale(&searched));
}

#[test]
fn a_reconcile_written_before_owners_reads_back_holding_no_repositories_and_writes_none() {
    let read: Reconciled = serde_json::from_str("{\"repository\":null,\"at\":null}").unwrap();
    assert_eq!(read, Reconciled::default());
    assert!(
        !serde_json::to_string(&read)
            .unwrap()
            .contains("repositories")
    );
    let owned = Reconciled {
        repositories: [RepositoryKey {
            host: "github.com".to_owned(),
            id: "1".to_owned(),
        }]
        .into(),
        ..Reconciled::default()
    };
    let written = serde_json::to_string(&owned).unwrap();
    assert_eq!(serde_json::from_str::<Reconciled>(&written).unwrap(), owned);
}

#[test]
fn what_a_delivery_says_of_who_may_see_a_pull_request_only_narrows_it() {
    let public = kept();
    let mut private = described(3_500);
    private.visibility = Some(Visibility::Private);
    let narrowed = KeptPull::fold(
        Some(public.clone()),
        key(),
        &Folded::Pull(Box::new(private)),
    )
    .unwrap();
    assert_eq!(narrowed.pull.visibility, Visibility::Private);
    let mut widening = described(3_600);
    widening.visibility = Some(Visibility::Public);
    let kept_private = KeptPull::fold(
        Some(narrowed.clone()),
        key(),
        &Folded::Pull(Box::new(widening.clone())),
    )
    .unwrap();
    assert_eq!(kept_private.pull.visibility, Visibility::Private);
    let begun = KeptPull::fold(None, key(), &Folded::Pull(Box::new(widening))).unwrap();
    assert_eq!(begun.pull.visibility, Visibility::Private);
    let internal = Folded::Visibility(Visibility::Internal);
    assert_eq!(internal.number(), None);
    let hidden =
        KeptPull::fold(Some(public.clone().vanished(at(4_000))), key(), &internal).unwrap();
    assert_eq!(hidden.pull.visibility, Visibility::Internal);
    let still = KeptPull::fold(
        Some(narrowed),
        key(),
        &Folded::Visibility(Visibility::Public),
    )
    .unwrap();
    assert_eq!(still.pull.visibility, Visibility::Private);
    assert_eq!(KeptPull::fold(None, key(), &internal), None);
    assert_eq!(public.clone().narrowed(Visibility::Public), public);
    assert_eq!(
        KeptPull::fold(
            Some(public.vanished(at(4_000))),
            key(),
            &check("abc123", "test", Outcome::Failed, 4_100)
        ),
        None
    );
}

#[test]
fn one_no_longer_listed_closes_once_how_unknown() {
    let closed = kept().vanished(at(4_000));
    assert_eq!(
        closed.closed,
        Some(Closed {
            at: at(4_000),
            merged: None,
        })
    );
    assert_eq!(closed.clone().vanished(at(5_000)), closed);
}

#[test]
fn a_delivery_describing_one_herald_keeps_none_of_begins_its_record() {
    let begun = KeptPull::fold(None, key(), &Folded::Pull(Box::new(described(2_500)))).unwrap();
    assert_eq!(begun.pull.author.as_deref(), Some("wessel"));
    assert_eq!(begun.pull.labels, ["queue"]);
    assert_eq!(begun.pull.review, Review::Requested);
    assert_eq!(begun.pull.checks, Checks::Absent);
    assert_eq!(begun.pull.visibility, Visibility::Private);
    assert_eq!(begun.since, Since::at(at(2_500)));
    assert_eq!(begun.read, None);
    let mut merged = described(2_600);
    merged.lifecycle = Lifecycle::Merged;
    let merged = KeptPull::fold(Some(begun), key(), &Folded::Pull(Box::new(merged))).unwrap();
    assert_eq!(
        merged.closed,
        Some(Closed {
            at: at(2_600),
            merged: Some(true),
        })
    );
    let mut closed = described(2_700);
    closed.lifecycle = Lifecycle::Closed;
    closed.author = None;
    let closed = KeptPull::fold(Some(merged), key(), &Folded::Pull(Box::new(closed))).unwrap();
    assert_eq!(closed.closed.unwrap().merged, Some(false));
    assert_eq!(closed.pull.author.as_deref(), Some("wessel"));
    for folded in [
        review("octo", Some(Verdict::Approved), Some(true), 1),
        check("abc123", "test", Outcome::Passed, 1),
        Folded::Activity {
            number: 7,
            at: at(1),
        },
    ] {
        assert_eq!(KeptPull::fold(None, key(), &folded), None);
    }
}

#[test]
fn a_newer_description_moves_the_record_and_an_older_one_is_left_aside() {
    let kept = folded(kept(), &[check("abc123", "test", Outcome::Failed, 3_100)]);
    let mut older = described(1_999);
    older.title = "older".to_owned();
    let left = KeptPull::fold(Some(kept.clone()), key(), &Folded::Pull(Box::new(older))).unwrap();
    assert_eq!(left, kept);
    let mut newer = described(3_500);
    newer.head = Some("def456".to_owned());
    newer.queued = Some(true);
    newer.visibility = Some(Visibility::Public);
    newer.forge = Some((Turn::WaitingOnReview, "blocked".to_owned()));
    newer.draft = true;
    let moved = KeptPull::fold(
        Some(kept.clone()),
        key(),
        &Folded::Pull(Box::new(newer.clone())),
    )
    .unwrap();
    assert_eq!(moved.pull.head.as_deref(), Some("def456"));
    assert!(moved.checks.is_empty());
    assert_eq!(moved.pull.checks, Checks::Absent);
    assert_eq!(moved.pull.mergeable, Mergeable::Unknown);
    assert!(moved.pull.queued && moved.pull.draft);
    assert_eq!(moved.pull.forge, newer.forge);
    assert_eq!(moved.pull.updated, at(3_500));
    assert_eq!(
        (moved.since.head, moved.since.queued, moved.since.draft),
        (at(3_500), at(3_500), at(3_500))
    );
    newer.head = None;
    newer.mergeable = Some(Mergeable::Conflict);
    newer.forge = None;
    let kept_head = KeptPull::fold(Some(moved), key(), &Folded::Pull(Box::new(newer))).unwrap();
    assert_eq!(kept_head.pull.head.as_deref(), Some("def456"));
    assert_eq!(kept_head.pull.mergeable, Mergeable::Conflict);
    assert_eq!(
        kept_head.pull.forge,
        Some((Turn::WaitingOnReview, "blocked".to_owned()))
    );
}

#[test]
fn a_deciding_change_request_outweighs_an_approval_and_an_unknown_weight_reads_unknown() {
    let mut requested = kept();
    requested.read.as_mut().unwrap().review = Review::Requested;
    requested.pull.review = Review::Requested;
    requested.pull.reviewers = vec!["octo".to_owned()];
    let approved = folded(
        requested.clone(),
        &[review("octo", Some(Verdict::Approved), Some(true), 3_100)],
    );
    assert_eq!(approved.pull.review, Review::Approved);
    assert_eq!(approved.since.review, at(3_100));
    assert_eq!(approved.pull.updated, at(3_100));
    let changes = folded(
        approved.clone(),
        &[review(
            "cat",
            Some(Verdict::ChangesRequested),
            Some(true),
            3_200,
        )],
    );
    assert_eq!(changes.pull.review, Review::ChangesRequested);
    let unknown = folded(
        approved.clone(),
        &[review("dog", Some(Verdict::Approved), None, 3_200)],
    );
    assert_eq!(unknown.pull.review, Review::Unknown);
    let outsider = folded(
        requested.clone(),
        &[review(
            "dog",
            Some(Verdict::ChangesRequested),
            Some(false),
            3_200,
        )],
    );
    assert_eq!(outsider.pull.review, Review::Requested);
    let dismissed = folded(approved.clone(), &[review("octo", None, Some(true), 3_300)]);
    assert_eq!(dismissed.pull.review, Review::Requested);
    let stale = folded(approved, &[review("octo", None, Some(true), 3_000)]);
    assert_eq!(stale.pull.review, Review::Approved);
    let mut unasked = requested;
    unasked.pull.reviewers.clear();
    assert_eq!(
        folded(unasked, &[review("dog", None, None, 1)]).pull.review,
        Review::Unasked
    );
    let mut blocked = kept();
    blocked.read.as_mut().unwrap().review = Review::ChangesRequested;
    let reapproved = folded(
        blocked,
        &[review("octo", Some(Verdict::Approved), Some(true), 3_100)],
    );
    assert_eq!(reapproved.pull.review, Review::Unknown);
}

#[test]
fn each_check_delivered_on_the_head_moves_what_its_checks_say() {
    let mut base = kept();
    base.read.as_mut().unwrap().checks = Checks::Failed(vec!["lint".to_owned()]);
    base.pull.checks = Checks::Failed(vec!["lint".to_owned()]);
    assert_eq!(
        KeptPull::fold(
            Some(base.clone()),
            key(),
            &check("other", "test", Outcome::Failed, 1)
        ),
        None
    );
    let failing = folded(
        base.clone(),
        &[check("abc123", "test", Outcome::Failed, 3_100)],
    );
    assert_eq!(
        failing.pull.checks,
        Checks::Failed(vec!["lint".to_owned(), "test".to_owned()])
    );
    let fixed = folded(base, &[check("abc123", "lint", Outcome::Passed, 3_100)]);
    assert_eq!(fixed.pull.checks, Checks::Pending);
    assert_eq!(fixed.since.checks, at(3_100));
    let green = folded(kept(), &[check("abc123", "test", Outcome::Passed, 3_100)]);
    assert_eq!(green.pull.checks, Checks::Passed);
    let running = folded(
        green.clone(),
        &[check("abc123", "lint", Outcome::Pending, 3_200)],
    );
    assert_eq!(running.pull.checks, Checks::Pending);
    let late = folded(green, &[check("abc123", "test", Outcome::Failed, 3_000)]);
    assert_eq!(late.pull.checks, Checks::Passed);
    let mut unread = kept();
    unread.read = None;
    let first = folded(unread, &[check("abc123", "test", Outcome::Passed, 3_100)]);
    assert_eq!(first.pull.checks, Checks::Passed);
}

#[test]
fn activity_moves_the_last_act_only_forward() {
    let kept = kept();
    let later = folded(
        kept.clone(),
        &[Folded::Activity {
            number: 7,
            at: at(5_000),
        }],
    );
    assert_eq!(later.pull.updated, at(5_000));
    let earlier = folded(
        kept,
        &[Folded::Activity {
            number: 7,
            at: at(1),
        }],
    );
    assert_eq!(earlier.pull.updated, at(2_000));
}

#[test]
fn a_delivery_names_the_pull_request_it_is_of_and_a_check_names_its_head_alone() {
    assert_eq!(Folded::Pull(Box::new(described(1))).number(), Some(7));
    assert_eq!(review("o", None, None, 1).number(), Some(7));
    assert_eq!(
        Folded::Activity {
            number: 8,
            at: at(1)
        }
        .number(),
        Some(8)
    );
    assert_eq!(check("abc", "t", Outcome::Passed, 1).number(), None);
}

#[test]
fn a_reconcile_is_due_every_round_or_once_its_interval_passed() {
    let hour = SignedDuration::from_hours(1);
    let listed = Reconciled {
        repositories: std::collections::BTreeSet::new(),
        repository: None,
        at: Some(at(3_600)),
        validators: Vec::new(),
    };
    assert!(Reconciled::due(None, Some(hour), at(0)));
    assert!(Reconciled::due(Some(&listed), None, at(3_600)));
    assert!(!Reconciled::due(Some(&listed), Some(hour), at(7_199)));
    assert!(Reconciled::due(Some(&listed), Some(hour), at(7_200)));
    let never = Reconciled { at: None, ..listed };
    assert!(Reconciled::due(Some(&never), Some(hour), at(0)));
}

#[test]
fn a_kept_pull_request_reads_back_as_it_was_written() {
    let kept = folded(
        kept(),
        &[
            check("abc123", "test", Outcome::Failed, 3_100),
            review("octo", Some(Verdict::ChangesRequested), Some(true), 3_200),
        ],
    );
    let written = serde_json::to_string(&kept).unwrap();
    assert_eq!(serde_json::from_str::<KeptPull>(&written).unwrap(), kept);
    assert!(written.contains("\"outcome\":\"failed\""));
    assert!(written.contains("\"verdict\":\"changes-requested\""));
}

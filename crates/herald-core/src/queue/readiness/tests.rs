use jiff::SignedDuration;

use super::*;
use crate::queue::tests::pull;
use crate::queue::{Reason, Turn};

fn glob(text: &str) -> Glob {
    Glob::new(text).unwrap()
}

fn release() -> Release {
    Release {
        milestone: Some(glob("v2.*")),
        label: None,
        branch: None,
        pull: None,
        combine: Combine::All,
        blocker_labels: BLOCKER_LABELS
            .iter()
            .map(|label| Glob::new(label).unwrap())
            .collect(),
        blocked_by: Blocker::ALL.to_vec(),
        withheld: Withheld::Unknown,
        header: true,
        rows: Rows::Pulls,
    }
}

fn row(pull: Pull) -> Row {
    Row {
        pull,
        state: Turn::WaitingOnMerge,
        reason: Reason::Ready,
        waited: SignedDuration::ZERO,
        stale: false,
        decided: std::collections::BTreeMap::new(),
        overlaps: Vec::new(),
    }
}

fn numbered(number: u64) -> Pull {
    Pull {
        number,
        milestone: Some("v2.0".to_owned()),
        ..pull()
    }
}

fn verdict(release: &Release, pulls: Vec<Pull>) -> Readiness {
    beside(release, pulls, Vec::new())
}

fn beside(release: &Release, pulls: Vec<Pull>, withheld: Vec<Pull>) -> Readiness {
    let rows: Vec<Row> = pulls.into_iter().map(row).collect();
    let hidden: Vec<Row> = withheld.into_iter().map(row).collect();
    let shown: Vec<&Row> = rows.iter().collect();
    let hidden: Vec<&Row> = hidden.iter().collect();
    release.verdict(
        Judging {
            rows: &shown,
            bases: &[],
        },
        Judging {
            rows: &hidden,
            bases: &[],
        },
    )
}

fn number(holding: &Holding) -> u64 {
    match &holding.holder {
        Holder::Pull { number, .. } => *number,
        Holder::Base { .. } => 0,
    }
}

fn shown(readiness: &Readiness) -> Vec<&Holding> {
    readiness
        .holds
        .iter()
        .map(|held| match held {
            Held::Shown(holding) => holding,
            Held::Private => panic!("a private hold"),
        })
        .collect()
}

fn reasons(readiness: &Readiness) -> Vec<(&'static str, u64, Vec<String>)> {
    shown(readiness)
        .into_iter()
        .map(|holding| {
            (
                holding.held_by.name(),
                number(holding),
                holding.named.clone(),
            )
        })
        .collect()
}

fn drafted(number: u64) -> Pull {
    Pull {
        draft: true,
        ..numbered(number)
    }
}

#[test]
fn a_milestone_picks_the_pull_requests_carrying_one_it_matches() {
    let release = release();
    assert!(release.picks(&numbered(1)));
    let other = Pull {
        milestone: Some("v1.9".to_owned()),
        ..pull()
    };
    assert!(!release.picks(&other));
    assert!(!release.picks(&pull()));
}

#[test]
fn a_label_and_a_branch_pick_by_any_label_carried_and_the_branch_merged_into() {
    let release = Release {
        milestone: None,
        label: Some(glob("release:*")),
        branch: Some(glob("release/*")),
        ..release()
    };
    let both = Pull {
        labels: vec!["bug".to_owned(), "Release:2.0".to_owned()],
        base: Some("release/2.0".to_owned()),
        ..pull()
    };
    assert!(release.picks(&both));
    let labelled = Pull {
        base: Some("main".to_owned()),
        ..both.clone()
    };
    assert!(!release.picks(&labelled));
    let unbased = Pull {
        base: None,
        ..both.clone()
    };
    assert!(!release.picks(&unbased));
    let branched = Pull {
        labels: vec!["bug".to_owned()],
        ..both
    };
    assert!(!release.picks(&branched));
}

#[test]
fn selectors_combined_by_any_pick_what_one_of_them_picks() {
    let release = Release {
        label: Some(glob("ship-it")),
        combine: Combine::Any,
        ..release()
    };
    let labelled = Pull {
        labels: vec!["ship-it".to_owned()],
        ..pull()
    };
    assert!(release.picks(&labelled));
    assert!(release.picks(&numbered(1)));
    assert!(!release.picks(&pull()));
    let all = Release {
        combine: Combine::All,
        ..release
    };
    assert!(!all.picks(&labelled));
    assert!(!all.picks(&numbered(1)));
    let carried = Pull {
        labels: vec!["ship-it".to_owned()],
        ..numbered(1)
    };
    assert!(all.picks(&carried));
}

#[test]
fn a_release_with_no_selector_picks_nothing() {
    for combine in [Combine::All, Combine::Any] {
        let release = Release {
            milestone: None,
            combine,
            ..release()
        };
        assert!(!release.picks(&numbered(1)));
    }
}

#[test]
fn a_release_pull_request_is_told_by_its_tool_s_head_branch_or_a_title() {
    let headed = |branch: &str| Pull {
        head_branch: Some(branch.to_owned()),
        ..pull()
    };
    let please = Release {
        milestone: None,
        pull: Some(ReleasePull::ReleasePlease),
        ..release()
    };
    assert!(please.picks(&headed("release-please--branches--main")));
    assert!(!please.picks(&headed("changeset-release/main")));
    assert!(!please.picks(&pull()));
    let changesets = Release {
        pull: Some(ReleasePull::Changesets),
        ..please.clone()
    };
    assert!(changesets.picks(&headed("changeset-release/main")));
    assert!(!changesets.picks(&headed("release-please--branches--main")));
    let titled = Release {
        pull: Some(ReleasePull::Title(Glob::new("chore: release *").unwrap())),
        ..please
    };
    let release_title = Pull {
        title: "chore: release 2.0.0".to_owned(),
        ..pull()
    };
    assert!(titled.picks(&release_title));
    assert!(!titled.picks(&pull()));
    let headed_glob = Release {
        pull: Some(ReleasePull::Head(glob("releases/*"))),
        ..titled
    };
    assert!(headed_glob.picks(&headed("releases/2.0")));
    assert!(!headed_glob.picks(&headed("main")));
    assert!(!headed_glob.picks(&pull()));
}

#[test]
fn nothing_holding_it_back_is_ready() {
    let readiness = verdict(&release(), vec![numbered(1), numbered(2)]);
    assert_eq!(readiness.ready(), Some(true));
    assert_eq!(readiness, Readiness::default());
    assert_eq!(verdict(&release(), Vec::new()).ready(), Some(true));
}

#[test]
fn each_blocker_holds_it_back_naming_the_labels_or_checks() {
    let labelled = Pull {
        labels: vec![
            "bug".to_owned(),
            "Blocker".to_owned(),
            "release-blocker".to_owned(),
        ],
        ..numbered(1)
    };
    let failed = Pull {
        checks: Checks::Failed(vec!["test".to_owned(), "lint".to_owned()]),
        ..numbered(2)
    };
    let draft = Pull {
        draft: true,
        ..numbered(3)
    };
    let changes = Pull {
        review: Review::ChangesRequested,
        ..numbered(4)
    };
    let readiness = verdict(
        &release(),
        vec![labelled, failed, draft, changes, numbered(5)],
    );
    assert_eq!(readiness.ready(), Some(false));
    assert_eq!(
        reasons(&readiness),
        vec![
            (
                "label",
                1,
                vec!["Blocker".to_owned(), "release-blocker".to_owned()]
            ),
            ("failed", 2, vec!["test".to_owned(), "lint".to_owned()]),
            ("draft", 3, Vec::new()),
            ("changes", 4, Vec::new()),
        ]
    );
    let first = shown(&readiness)[0];
    assert_eq!(first.repository, "nightworksio/herald");
    assert_eq!(
        first.holder,
        Holder::Pull {
            number: 1,
            url: "https://github.com/nightworksio/herald/pull/7".to_owned()
        }
    );
    assert_eq!(first.more, 0);
}

#[test]
fn one_pull_request_holds_it_back_once_for_each_blocker() {
    let both = Pull {
        draft: true,
        review: Review::ChangesRequested,
        ..numbered(1)
    };
    assert_eq!(
        reasons(&verdict(&release(), vec![both])),
        vec![("draft", 1, Vec::new()), ("changes", 1, Vec::new())]
    );
}

#[test]
fn only_the_blockers_named_hold_it_back() {
    let release = Release {
        blocked_by: vec![Blocker::Draft],
        ..release()
    };
    let failed = Pull {
        checks: Checks::Failed(vec!["test".to_owned()]),
        labels: vec!["blocker".to_owned()],
        review: Review::ChangesRequested,
        ..numbered(1)
    };
    let draft = Pull {
        draft: true,
        ..numbered(2)
    };
    assert_eq!(
        reasons(&verdict(&release, vec![failed.clone(), draft])),
        vec![("draft", 2, Vec::new())]
    );
    let none = Release {
        blocked_by: Vec::new(),
        ..release
    };
    assert_eq!(verdict(&none, vec![failed]).ready(), Some(true));
}

#[test]
fn checks_pending_absent_or_passed_and_other_labels_hold_nothing_back() {
    let pulls = [Checks::Absent, Checks::Pending, Checks::Passed].map(|checks| Pull {
        checks,
        labels: vec!["not-a-blocker".to_owned()],
        review: Review::Requested,
        ..numbered(1)
    });
    assert_eq!(verdict(&release(), pulls.to_vec()).ready(), Some(true));
}

#[test]
fn a_pull_request_not_read_in_full_holds_it_back_whatever_blocks() {
    let unread = Pull {
        unread: true,
        draft: true,
        ..numbered(1)
    };
    let release = Release {
        blocked_by: Vec::new(),
        ..release()
    };
    assert_eq!(
        reasons(&verdict(&release, vec![unread])),
        vec![("unread", 1, Vec::new())]
    );
}

#[test]
fn a_verdict_names_at_most_its_bound_and_counts_the_rest() {
    let pulls: Vec<Pull> = (1..=u64::try_from(MOST_HOLDS).unwrap() + 2)
        .map(drafted)
        .collect();
    let readiness = verdict(&release(), pulls);
    assert_eq!(readiness.holds.len(), MOST_HOLDS);
    assert_eq!(readiness.more, 2);
    assert_eq!(
        shown(&readiness).last().map(|holding| number(holding)),
        Some(u64::try_from(MOST_HOLDS).unwrap())
    );
}

#[test]
fn a_hold_names_at_most_its_bound_of_checks_and_counts_the_rest() {
    let checks: Vec<String> = (0..MOST_HELD_CHECKS + 3)
        .map(|index| format!("check-{index}"))
        .collect();
    let failed = Pull {
        checks: Checks::Failed(checks.clone()),
        ..numbered(1)
    };
    let readiness = verdict(&release(), vec![failed]);
    let holding = shown(&readiness)[0];
    assert_eq!(holding.named, checks[..MOST_HELD_CHECKS].to_vec());
    assert_eq!(holding.more, 3);
}

#[test]
fn blockers_and_reasons_have_their_names() {
    let names: Vec<&str> = Blocker::ALL.iter().map(|blocker| blocker.name()).collect();
    assert_eq!(names, ["label", "failed", "draft", "changes", "base"]);
    assert_eq!(HeldBy::Unread.name(), "unread");
    assert_eq!(HeldBy::Blocked(Blocker::Changes).name(), "changes");
}

#[test]
fn a_release_some_of_whose_pull_requests_are_withheld_is_not_known_by_default_unless_a_shown_one_holds_it()
 {
    let release = release();
    let readiness = beside(&release, vec![numbered(1)], vec![numbered(2)]);
    assert_eq!(readiness.ready(), None);
    assert_eq!(readiness.judged, Judged::Unknown);
    assert_eq!(readiness.holds, []);
    let held = beside(&release, vec![drafted(1)], vec![drafted(2)]);
    assert_eq!(held.ready(), Some(false));
    assert_eq!(held.judged, Judged::Whole);
    assert_eq!(reasons(&held), vec![("draft", 1, Vec::new())]);
    let whole = beside(&release, vec![numbered(1)], Vec::new());
    assert_eq!(whole.ready(), Some(true));
    assert_eq!(whole.judged, Judged::Whole);
}

#[test]
fn a_release_judged_by_what_is_shown_says_nothing_of_what_is_withheld() {
    let release = Release {
        withheld: Withheld::Shown,
        ..release()
    };
    let readiness = beside(&release, vec![numbered(1)], vec![drafted(2)]);
    assert_eq!(readiness.ready(), Some(true));
    assert_eq!(readiness.judged, Judged::Shown);
    assert_eq!(readiness.holds, []);
    let held = beside(&release, vec![drafted(1)], vec![drafted(2)]);
    assert_eq!(held.ready(), Some(false));
    assert_eq!(reasons(&held), vec![("draft", 1, Vec::new())]);
    assert_eq!(
        beside(&release, vec![numbered(1)], Vec::new()).judged,
        Judged::Whole
    );
}

#[test]
fn a_masked_release_names_each_withheld_pull_request_holding_it_private_once_after_the_shown() {
    let release = Release {
        withheld: Withheld::Masked,
        ..release()
    };
    let twice = Pull {
        review: Review::ChangesRequested,
        labels: vec!["blocker".to_owned()],
        ..drafted(1)
    };
    let unread = Pull {
        unread: true,
        ..numbered(2)
    };
    let readiness = beside(&release, vec![drafted(9)], vec![twice, numbered(3), unread]);
    assert_eq!(readiness.ready(), Some(false));
    assert_eq!(readiness.judged, Judged::Whole);
    let Held::Shown(first) = &readiness.holds[0] else {
        panic!("a private hold first");
    };
    assert_eq!(number(first), 9);
    assert_eq!(&readiness.holds[1..], [Held::Private, Held::Private]);
    let clear = beside(&release, vec![numbered(1)], vec![numbered(2)]);
    assert_eq!(clear.ready(), Some(true));
    assert_eq!(clear, Readiness::default());
}

#[test]
fn private_holds_count_within_the_bound_of_holds() {
    let release = Release {
        withheld: Withheld::Masked,
        ..release()
    };
    let most = u64::try_from(MOST_HOLDS).unwrap();
    let under: Vec<Pull> = (1..most).map(drafted).collect();
    let readiness = beside(&release, under.clone(), vec![drafted(most)]);
    assert_eq!(readiness.holds.len(), MOST_HOLDS);
    assert_eq!(readiness.more, 0);
    assert_eq!(readiness.holds.last(), Some(&Held::Private));
    let over = beside(&release, under, vec![drafted(most), drafted(most + 1)]);
    assert_eq!(over.holds.len(), MOST_HOLDS);
    assert_eq!(over.more, 1);
}

#[test]
fn a_release_reads_its_milestones_only_where_it_shows_a_header_or_lists_their_issues() {
    let glob = glob("v2.*");
    let heading = |header: bool, issues: bool| Some((&glob, HeaderRead { header, issues }));
    assert_eq!(release().heading(), heading(true, false));
    let unheaded = Release {
        header: false,
        rows: Rows::Pulls,
        ..release()
    };
    assert_eq!(unheaded.heading(), None);
    let listed = Release {
        rows: Rows::Issues,
        ..unheaded.clone()
    };
    assert_eq!(listed.heading(), heading(false, true));
    let both = Release {
        rows: Rows::Issues,
        ..release()
    };
    assert_eq!(both.heading(), heading(true, true));
    let unmilestoned = Release {
        milestone: None,
        ..both
    };
    assert_eq!(unmilestoned.heading(), None);
}

fn base(repository: &str, read: BaseRead) -> Based {
    Based {
        repository: repository.to_owned(),
        visibility: crate::Visibility::Private,
        branch: "main".to_owned(),
        read,
    }
}

fn failing(names: usize) -> BaseRead {
    BaseRead::Checked(Checks::Failed(
        (0..names).map(|index| format!("c{index}")).collect(),
    ))
}

fn based_verdict(
    release: &Release,
    (pulls, bases): (Vec<Pull>, &[Based]),
    (withheld, hidden): (Vec<Pull>, &[Based]),
) -> Readiness {
    let rows: Vec<Row> = pulls.into_iter().map(row).collect();
    let unshown: Vec<Row> = withheld.into_iter().map(row).collect();
    let (rows, unshown): (Vec<&Row>, Vec<&Row>) = (rows.iter().collect(), unshown.iter().collect());
    let (bases, hidden): (Vec<&Based>, Vec<&Based>) =
        (bases.iter().collect(), hidden.iter().collect());
    release.verdict(
        Judging {
            rows: &rows,
            bases: &bases,
        },
        Judging {
            rows: &unshown,
            bases: &hidden,
        },
    )
}

#[test]
fn a_base_branch_with_a_failed_check_holds_it_back_naming_each_check() {
    let bases = [
        base("o/a", failing(MOST_HELD_CHECKS + 2)),
        base("o/b", BaseRead::Checked(Checks::Passed)),
        base("o/c", BaseRead::Checked(Checks::Pending)),
        base("o/d", BaseRead::Checked(Checks::Absent)),
        base("o/e", BaseRead::Unread("refused".to_owned())),
        base("o/f", failing(MOST_HELD_CHECKS)),
    ];
    let readiness = based_verdict(&release(), (Vec::new(), &bases), (Vec::new(), &[]));
    assert_eq!(readiness.ready(), Some(false));
    let holds = shown(&readiness);
    let told: Vec<(&str, &str, &Holder, usize, usize)> = holds
        .iter()
        .map(|holding| {
            (
                holding.held_by.name(),
                holding.repository.as_str(),
                &holding.holder,
                holding.named.len(),
                holding.more,
            )
        })
        .collect();
    let main = Holder::Base {
        branch: "main".to_owned(),
    };
    assert_eq!(
        told,
        [
            ("base", "o/a", &main, MOST_HELD_CHECKS, 2),
            ("unread", "o/e", &main, 1, 0),
            ("base", "o/f", &main, MOST_HELD_CHECKS, 0),
        ]
    );
    assert_eq!(holds[1].named, ["refused"]);
}

#[test]
fn a_release_not_blocked_by_base_branches_is_not_held_by_one() {
    let unbased = Release {
        blocked_by: vec![Blocker::Failed],
        ..release()
    };
    assert!(!unbased.reads_bases());
    assert!(release().reads_bases());
    let bases = [
        base("o/a", failing(1)),
        base("o/b", BaseRead::Unread("refused".to_owned())),
    ];
    let readiness = based_verdict(&unbased, (Vec::new(), &bases), (Vec::new(), &bases));
    assert_eq!(readiness.ready(), None);
    let masked = Release {
        withheld: Withheld::Masked,
        ..unbased
    };
    let readiness = based_verdict(&masked, (Vec::new(), &[]), (Vec::new(), &bases));
    assert_eq!(readiness, Readiness::default());
    assert_eq!(Blocker::Base.name(), "base");
    assert_eq!(Blocker::ALL.len(), 5);
}

#[test]
fn a_withheld_base_branch_is_told_as_withheld_says() {
    let withheld = [base("o/vault", failing(1))];
    let clear = [base("o/vault", BaseRead::Checked(Checks::Passed))];
    let told = |withheld_as, hidden: &[Based], shown: &[Based]| {
        let release = Release {
            withheld: withheld_as,
            ..release()
        };
        based_verdict(&release, (vec![numbered(1)], shown), (Vec::new(), hidden))
    };
    let unknown = told(Withheld::Unknown, &withheld, &[]);
    assert_eq!((unknown.ready(), unknown.judged), (None, Judged::Unknown));
    assert_eq!(told(Withheld::Unknown, &clear, &[]).ready(), None);
    let held = told(Withheld::Unknown, &withheld, &[base("o/a", failing(1))]);
    assert_eq!((held.ready(), held.judged), (Some(false), Judged::Whole));
    let by_shown = told(Withheld::Shown, &withheld, &[]);
    assert_eq!(
        (by_shown.ready(), by_shown.judged),
        (Some(true), Judged::Shown)
    );
    let masked = told(Withheld::Masked, &withheld, &[]);
    assert_eq!(masked.holds, [Held::Private]);
    assert_eq!(masked.ready(), Some(false));
    let masked_clear = told(Withheld::Masked, &clear, &[]);
    assert_eq!(masked_clear.ready(), Some(true));
    let both = based_verdict(
        &Release {
            withheld: Withheld::Masked,
            ..release()
        },
        (Vec::new(), &[]),
        (vec![drafted(2)], &withheld),
    );
    assert_eq!(both.holds, [Held::Private, Held::Private]);
}

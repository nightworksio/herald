use super::*;
use crate::bump::Bump;
use crate::queue::area::{Area, AreaKind, of_paths};
use crate::queue::tests::pull;
use crate::queue::{Checks, Mergeable, Review};

const DAY: i64 = 24 * 60 * 60;

fn now() -> Timestamp {
    Timestamp::from_second(100 * DAY).unwrap()
}

fn waited(days: i64, number: u64) -> Pull {
    Pull {
        number,
        updated: Timestamp::from_second((100 - days) * DAY).unwrap(),
        ..pull()
    }
}

fn globs(texts: &[&str]) -> Vec<Glob> {
    texts.iter().map(|text| Glob::new(text).unwrap()).collect()
}

fn numbers(board: &Board) -> Vec<Vec<u64>> {
    board
        .groups
        .iter()
        .map(|group| group.rows.iter().map(|row| row.pull.number).collect())
        .collect()
}

fn names(board: &Board) -> Vec<&str> {
    board
        .groups
        .iter()
        .map(|group| group.name.as_str())
        .collect()
}

fn bot(title: &str, repository: &str, number: u64) -> Pull {
    Pull {
        title: title.to_owned(),
        repository: repository.to_owned(),
        bot: true,
        author: Some("dependabot[bot]".to_owned()),
        ..waited(1, number)
    }
}

#[test]
fn a_default_view_is_stale_after_fourteen_days_and_batches_bots() {
    let view = View::default();
    assert_eq!(view.stale, SignedDuration::from_hours(14 * 24));
    assert_eq!(view.bots, Bots::Batch);
    assert_eq!(view.group_by, GroupBy::Turn);
    assert_eq!(view.deciding, Deciding::Herald);
    assert_eq!(view.rules, Rules::default());
    assert!(!view.stale_only);
    assert!(view.states.is_empty() && view.bumps.is_empty());
}

#[test]
fn rows_are_grouped_by_turn_in_the_order_a_view_lists_turns_oldest_first() {
    let pulls = [
        Pull {
            review: Review::Requested,
            ..waited(2, 1)
        },
        Pull {
            checks: Checks::Pending,
            ..waited(1, 2)
        },
        waited(3, 3),
        Pull {
            review: Review::Requested,
            ..waited(5, 4)
        },
        Pull {
            draft: true,
            ..waited(0, 5)
        },
    ];
    let board = board(&View::default(), &pulls, now());
    assert_eq!(
        names(&board),
        [
            "waiting-on-review",
            "waiting-on-merge",
            "waiting-on-checks",
            "draft"
        ]
    );
    assert_eq!(numbers(&board), [vec![4, 1], vec![3], vec![2], vec![5]]);
    let first = board.groups.first().unwrap().rows.first().unwrap();
    assert_eq!(first.waited, SignedDuration::from_secs(5 * DAY));
    assert_eq!(first.reason, Reason::ReviewRequested);
    assert_eq!(board.total(), 5);
    let counted: Vec<usize> = board.counts.iter().map(|(_, count)| *count).collect();
    assert_eq!(counted, [2, 1, 0, 1, 0, 1, 0]);
    let turns: Vec<Turn> = board.counts.iter().map(|(turn, _)| *turn).collect();
    assert_eq!(turns, Turn::ALL);
}

#[test]
fn equal_waits_are_ordered_by_repository_then_number() {
    let pulls = [
        Pull {
            repository: "b/b".to_owned(),
            ..waited(1, 1)
        },
        Pull {
            repository: "a/a".to_owned(),
            ..waited(1, 9)
        },
        Pull {
            repository: "a/a".to_owned(),
            ..waited(1, 3)
        },
    ];
    assert_eq!(
        numbers(&board(&View::default(), &pulls, now())),
        [vec![3, 9, 1]]
    );
}

#[test]
fn a_pull_updated_after_now_has_waited_nothing() {
    let pulls = [waited(-1, 1)];
    let board = board(&View::default(), &pulls, now());
    let row = board.groups.first().unwrap().rows.first().unwrap();
    assert_eq!(row.waited, SignedDuration::ZERO);
    assert!(!row.stale);
}

#[test]
fn a_pull_is_stale_from_the_moment_it_waited_its_view_s_stale() {
    let view = View {
        stale: SignedDuration::from_secs(10 * DAY),
        ..View::default()
    };
    let pulls = [waited(10, 1), waited(9, 2), waited(11, 3)];
    let board = board(&view, &pulls, now());
    assert_eq!(board.stale, 2);
    let stale: Vec<(u64, bool)> = board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| (row.pull.number, row.stale))
        .collect();
    assert_eq!(stale, [(3, true), (1, true), (2, false)]);
    let only = View {
        stale_only: true,
        ..view
    };
    assert_eq!(numbers(&board_of(&only, &pulls)), [vec![3, 1]]);
}

fn board_of(view: &View, pulls: &[Pull]) -> Board {
    board(view, pulls, now())
}

#[test]
fn a_view_shows_only_the_turns_it_names() {
    let view = View {
        states: vec![Turn::WaitingOnAuthor],
        ..View::default()
    };
    let pulls = [
        waited(1, 1),
        Pull {
            mergeable: Mergeable::Conflict,
            ..waited(1, 2)
        },
    ];
    let board = board_of(&view, &pulls);
    assert_eq!(numbers(&board), [vec![2]]);
    assert_eq!(board.total(), 1);
}

#[test]
fn repositories_authors_and_labels_narrow_by_glob() {
    let pulls = [
        Pull {
            repository: "nightworksio/herald".to_owned(),
            author: Some("wessel".to_owned()),
            labels: vec!["area/core".to_owned(), "big".to_owned()],
            ..waited(1, 1)
        },
        Pull {
            repository: "lemonfiber/lemonfiber".to_owned(),
            author: None,
            ..waited(1, 2)
        },
    ];
    let narrowed = |view: View| numbers(&board_of(&view, &pulls));
    assert_eq!(narrowed(View::default()), [vec![2, 1]]);
    assert_eq!(
        narrowed(View {
            repositories: globs(&["lemonfiber/*"]),
            ..View::default()
        }),
        [vec![2]]
    );
    assert_eq!(
        narrowed(View {
            authors: globs(&["wes*"]),
            ..View::default()
        }),
        [vec![1]]
    );
    assert_eq!(
        narrowed(View {
            labels: globs(&["area/*"]),
            ..View::default()
        }),
        [vec![1]]
    );
    assert_eq!(
        narrowed(View {
            labels: globs(&["none"]),
            ..View::default()
        }),
        Vec::<Vec<u64>>::new()
    );
}

#[test]
fn bots_are_included_excluded_listed_alone_or_batched() {
    let pulls = [waited(1, 1), bot("Bump a from 1 to 2", "x/y", 2)];
    let with = |bots: Bots| {
        let board = board_of(
            &View {
                bots,
                ..View::default()
            },
            &pulls,
        );
        (numbers(&board), board.batches.len())
    };
    assert_eq!(with(Bots::Include), (vec![vec![1, 2]], 0));
    assert_eq!(with(Bots::Exclude), (vec![vec![1]], 0));
    assert_eq!(with(Bots::Only), (vec![vec![2]], 0));
    assert_eq!(with(Bots::Batch), (vec![vec![1]], 1));
}

#[test]
fn the_same_bump_in_every_repository_is_one_batch_counting_green_and_failing() {
    let pulls = [
        bot("Bump serde from 1 to 2", "a/one", 1),
        Pull {
            checks: Checks::Failed(vec!["test".to_owned()]),
            ..bot("chore(deps): bump serde from 1.1 to 2", "a/two", 2)
        },
        Pull {
            checks: Checks::Pending,
            ..bot("Bump serde from 1 to 2", "a/three", 3)
        },
        bot("Bump jiff from 0.2 to 0.3", "a/one", 4),
        bot("Refresh the lockfile", "a/one", 5),
    ];
    let board = board_of(&View::default(), &pulls);
    assert_eq!(board.groups, []);
    assert_eq!(board.total(), 5);
    let batches: Vec<(Option<Bump>, Vec<u64>, usize, usize)> = board
        .batches
        .iter()
        .map(|batch| {
            (
                batch.bump.clone(),
                batch.rows.iter().map(|row| row.pull.number).collect(),
                batch.green,
                batch.failing,
            )
        })
        .collect();
    let bump = |package: &str, to: &str| {
        Some(Bump {
            package: package.to_owned(),
            to: Some(to.to_owned()),
        })
    };
    assert_eq!(
        batches,
        [
            (bump("serde", "2"), vec![1, 3, 2], 1, 1),
            (None, vec![5], 1, 0),
            (bump("jiff", "0.3"), vec![4], 1, 0),
        ]
    );
}

#[test]
fn a_view_s_own_bump_patterns_read_before_herald_s() {
    let view = View {
        bumps: vec![BumpPattern::new("Bump {package} *").unwrap()],
        ..View::default()
    };
    let pulls = [bot("Bump serde from 1 to 2", "a/one", 1)];
    let board = board_of(&view, &pulls);
    assert_eq!(
        board.batches.first().unwrap().bump,
        Some(Bump {
            package: "serde".to_owned(),
            to: None,
        })
    );
}

#[test]
fn rows_group_by_repository_author_or_nothing() {
    let pulls = [
        Pull {
            repository: "b/b".to_owned(),
            author: Some("ann".to_owned()),
            ..waited(1, 1)
        },
        Pull {
            repository: "a/a".to_owned(),
            author: None,
            mergeable: Mergeable::Conflict,
            ..waited(2, 2)
        },
        Pull {
            repository: "b/b".to_owned(),
            author: Some("ann".to_owned()),
            review: Review::Requested,
            ..waited(3, 3)
        },
    ];
    let grouped = |group_by: GroupBy| {
        let board = board_of(
            &View {
                group_by,
                ..View::default()
            },
            &pulls,
        );
        (
            names(&board)
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>(),
            numbers(&board),
        )
    };
    assert_eq!(
        grouped(GroupBy::Repository),
        (
            vec!["a/a".to_owned(), "b/b".to_owned()],
            vec![vec![2], vec![3, 1]]
        )
    );
    assert_eq!(
        grouped(GroupBy::Author),
        (
            vec![String::new(), "ann".to_owned()],
            vec![vec![2], vec![3, 1]]
        )
    );
    assert_eq!(
        grouped(GroupBy::Nothing),
        (vec![String::new()], vec![vec![3, 2, 1]])
    );
}

#[test]
fn rows_group_under_each_of_their_areas_and_a_view_narrows_by_area() {
    let pulls = [
        Pull {
            areas: of_paths(&["api", "web"]),
            ..waited(1, 1)
        },
        Pull {
            areas: of_paths(&["docs"]),
            ..waited(2, 2)
        },
        waited(3, 3),
    ];
    let grouped = |group_by: GroupBy, areas: &[&str]| {
        let board = board_of(
            &View {
                group_by,
                areas: globs(areas),
                ..View::default()
            },
            &pulls,
        );
        let names: Vec<String> = names(&board).into_iter().map(str::to_owned).collect();
        (names, numbers(&board), board.total())
    };
    assert_eq!(
        grouped(GroupBy::Area, &[]),
        (
            vec![
                String::new(),
                "api".to_owned(),
                "docs".to_owned(),
                "web".to_owned()
            ],
            vec![vec![3], vec![1], vec![2], vec![1]],
            3
        )
    );
    assert_eq!(
        grouped(GroupBy::Nothing, &["W*", "docs"]),
        (vec![String::new()], vec![vec![2, 1]], 2)
    );
    assert_eq!(
        grouped(GroupBy::Area, &["none"]),
        (Vec::new(), Vec::new(), 0)
    );
}

#[test]
fn a_board_s_distinct_rows_hold_each_pull_request_once_however_many_groups_show_it() {
    let pulls = [
        Pull {
            areas: of_paths(&["api", "web"]),
            ..waited(1, 1)
        },
        Pull {
            url: "https://gitlab.com/o/r/-/merge_requests/1".to_owned(),
            areas: of_paths(&["web"]),
            ..waited(2, 1)
        },
        bot("chore(deps): bump serde from 1.0.0 to 1.0.1", "o/r", 2),
    ];
    let view = View {
        group_by: GroupBy::Area,
        ..View::default()
    };
    let shown = board_of(&view, &pulls);
    assert_eq!(numbers(&shown), [vec![1], vec![1, 1]]);
    let distinct: Vec<(u64, &str)> = shown
        .distinct_rows()
        .into_iter()
        .map(|row| (row.pull.number, row.pull.url.as_str()))
        .collect();
    assert_eq!(distinct.len(), 3, "{distinct:?}");
    assert_eq!(shown.total(), 3);
    assert_eq!(Board::default().distinct_rows(), Vec::<&Row>::new());
}

#[test]
fn a_pull_request_two_owners_own_is_one_row_under_each_and_counted_once() {
    let owned = |name: &str, kind: AreaKind| Area {
        name: name.to_owned(),
        kind,
    };
    let pulls = [Pull {
        areas: vec![
            owned("@alice", AreaKind::Person),
            owned("@org/web", AreaKind::Team),
            owned("@org/web", AreaKind::Section),
        ],
        ..waited(1, 1)
    }];
    let view = View {
        group_by: GroupBy::Area,
        ..View::default()
    };
    let shown = board_of(&view, &pulls);
    assert_eq!(names(&shown), ["@alice", "@org/web"]);
    assert_eq!(numbers(&shown), [vec![1], vec![1]]);
    assert_eq!(shown.distinct_rows().len(), 1);
    assert_eq!(shown.total(), 1);
    let narrowed = board_of(
        &View {
            areas: globs(&["@alice"]),
            ..View::default()
        },
        &pulls,
    );
    assert_eq!(narrowed.total(), 1);
}

#[test]
fn the_forge_or_labels_tell_whose_turn_where_a_view_says() {
    let pulls = [
        Pull {
            forge: Some((Turn::Queued, "in_train".to_owned())),
            labels: vec!["needs-work".to_owned()],
            ..waited(1, 1)
        },
        waited(2, 2),
    ];
    let forge = board_of(
        &View {
            deciding: Deciding::Forge,
            ..View::default()
        },
        &pulls,
    );
    assert_eq!(names(&forge), ["queued", "unknown"]);
    assert_eq!(
        forge.groups.first().unwrap().rows.first().unwrap().reason,
        Reason::Forge("in_train".to_owned())
    );
    let labelled = board_of(
        &View {
            deciding: Deciding::Labels(vec![("needs-work".to_owned(), Turn::WaitingOnAuthor)]),
            ..View::default()
        },
        &pulls,
    );
    assert_eq!(names(&labelled), ["waiting-on-author", "unknown"]);
}

#[test]
fn a_view_s_rules_tell_whose_turn_by_herald_s_own() {
    let view = View {
        rules: Rules {
            approval: true,
            ..Rules::default()
        },
        ..View::default()
    };
    let pulls = [Pull {
        review: Review::Unasked,
        ..waited(1, 1)
    }];
    assert_eq!(names(&board_of(&view, &pulls)), ["waiting-on-review"]);
    let unrequired = View {
        rules: Rules {
            approval: false,
            ..Rules::default()
        },
        ..View::default()
    };
    assert_eq!(names(&board_of(&unrequired, &pulls)), ["waiting-on-merge"]);
}

#[test]
fn a_row_waits_on_its_author_its_reviewers_or_nobody_by_its_state() {
    let asked = Pull {
        reviewers: vec!["ana".to_owned()],
        ..pull()
    };
    let cases = [
        (Turn::Draft, vec!["wessel"]),
        (Turn::WaitingOnAuthor, vec!["wessel"]),
        (Turn::WaitingOnReview, vec!["ana"]),
        (Turn::WaitingOnMerge, Vec::new()),
        (Turn::WaitingOnChecks, Vec::new()),
        (Turn::Queued, Vec::new()),
        (Turn::Unknown, Vec::new()),
    ];
    for (state, logins) in cases {
        let row = Row {
            pull: asked.clone(),
            state,
            reason: Reason::Untold,
            waited: SignedDuration::ZERO,
            stale: false,
            decided: BTreeMap::new(),
            overlaps: Vec::new(),
        };
        assert_eq!(row.waiting_on(), logins, "{state:?}");
    }
    let authorless = Row {
        pull: Pull {
            author: None,
            ..asked
        },
        state: Turn::WaitingOnAuthor,
        reason: Reason::Untold,
        waited: SignedDuration::ZERO,
        stale: false,
        decided: BTreeMap::new(),
        overlaps: Vec::new(),
    };
    assert_eq!(authorless.waiting_on(), Vec::<&str>::new());
}

#[test]
fn each_row_names_the_open_pull_requests_changing_a_path_it_changes_even_those_it_hides() {
    let changing = |number: u64, bot: bool| Pull {
        number,
        bot,
        base: Some("main".to_owned()),
        files: Some(crate::queue::Changed {
            paths: ["src/lib.rs".to_owned()].into(),
            whole: true,
        }),
        ..waited(1, number)
    };
    let view = View {
        bots: Bots::Exclude,
        ..View::default()
    };
    let shown = rows(&view, &[changing(1, false), changing(2, true)], now());
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].overlaps, [2]);
    let alone = rows(&View::default(), &[waited(1, 3)], now());
    assert_eq!(alone[0].overlaps, Vec::<u64>::new());
}

fn release(milestone: &str) -> crate::queue::Release {
    crate::queue::Release {
        milestone: Some(Glob::new(milestone).unwrap()),
        label: None,
        branch: None,
        pull: None,
        combine: crate::queue::Combine::All,
        blocker_labels: Vec::new(),
        blocked_by: crate::queue::Blocker::ALL.to_vec(),
    }
}

#[test]
fn a_release_shows_only_the_pull_requests_it_picks_and_judges_them_in_order() {
    let view = View {
        release: Some(release("v2.0")),
        ..View::default()
    };
    let carried = |repository: &str, number: u64, draft: bool| Pull {
        repository: repository.to_owned(),
        milestone: Some("v2.0".to_owned()),
        draft,
        ..waited(1, number)
    };
    let pulls = [
        carried("b/b", 1, true),
        waited(2, 2),
        carried("a/a", 9, true),
        carried("a/a", 3, false),
    ];
    let board = board(&view, &pulls, now());
    assert_eq!(board.total(), 3);
    let readiness = board.readiness.unwrap();
    let held: Vec<(&str, u64)> = readiness
        .holds
        .iter()
        .map(|holding| (holding.repository.as_str(), holding.number))
        .collect();
    assert_eq!(held, [("a/a", 9), ("b/b", 1)]);
}

#[test]
fn a_view_with_no_release_shows_no_verdict() {
    let board = board(&View::default(), &[waited(1, 1)], now());
    assert_eq!(board.readiness, None);
    assert_eq!(board.total(), 1);
}

use jiff::Timestamp;

use super::*;
use crate::members::Visibility;
use crate::queue::tests::pull;
use crate::queue::{Review, View, board};

const HOUR: i64 = 3_600;
const DAY: i64 = 24 * HOUR;

fn hours(count: i64) -> SignedDuration {
    SignedDuration::from_hours(count)
}

fn stages() -> Stages {
    Stages::new(vec![hours(7 * 24), hours(2 * 24)]).unwrap()
}

#[test]
fn stages_are_one_or_more_whole_distinct_lengths_shortest_first() {
    assert_eq!(stages().as_slice(), [hours(48), hours(168)]);
    assert_eq!(Stages::new(Vec::new()), None);
    assert_eq!(Stages::new(vec![hours(2), hours(2)]), None);
    assert_eq!(Stages::new(vec![SignedDuration::ZERO]), None);
    assert_eq!(Stages::new(vec![SignedDuration::from_millis(1_500)]), None);
    assert_eq!(Stages::new(vec![SignedDuration::from_secs(-60)]), None);
    let most: Vec<SignedDuration> = (1..=16).map(hours).collect();
    assert!(Stages::new(most.clone()).is_some());
    let more: Vec<SignedDuration> = (1..=17).map(hours).collect();
    assert_eq!(Stages::new(more), None);
    assert!(Stages::new(vec![SignedDuration::from_secs(1)]).is_some());
}

#[test]
fn a_wait_reaches_the_longest_stage_it_passed() {
    let stages = stages();
    assert_eq!(stages.reached(hours(47)), None);
    assert_eq!(stages.reached(hours(48)), Some(hours(48)));
    assert_eq!(stages.reached(hours(167)), Some(hours(48)));
    assert_eq!(stages.reached(hours(168)), Some(hours(168)));
    assert_eq!(stages.reached(hours(1_000)), Some(hours(168)));
}

#[test]
fn a_stage_is_written_in_its_largest_whole_unit() {
    let written: Vec<String> = [
        SignedDuration::from_secs(14 * DAY),
        SignedDuration::from_secs(2 * DAY),
        SignedDuration::from_secs(36 * HOUR),
        SignedDuration::from_secs(90 * 60),
        SignedDuration::from_secs(90),
    ]
    .into_iter()
    .map(stage_name)
    .collect();
    assert_eq!(written, ["2w", "2d", "36h", "90m", "90s"]);
}

#[test]
fn a_pull_is_named_by_its_page_s_host_or_else_its_project() {
    assert_eq!(
        pull_identity(&pull()),
        "github.com/nightworksio/herald/pull/7"
    );
    let pageless = Pull {
        url: "not a page".to_owned(),
        ..pull()
    };
    assert_eq!(
        pull_identity(&pageless),
        "herald/nightworksio/herald/pull/7"
    );
}

fn waiting(number: u64, updated: i64) -> Pull {
    Pull {
        number,
        url: format!("https://github.com/nightworksio/herald/pull/{number}"),
        review: Review::Requested,
        reviewers: vec!["ana".to_owned(), "org/core".to_owned()],
        updated: Timestamp::from_second(updated).unwrap(),
        ..pull()
    }
}

fn now() -> Timestamp {
    Timestamp::from_second(100 * DAY).unwrap()
}

#[test]
fn a_rule_nudges_each_row_past_a_stage_the_longest_waiting_first_up_to_its_most() {
    let pulls = [
        waiting(1, 99 * DAY),
        waiting(2, 97 * DAY),
        waiting(3, 90 * DAY),
        waiting(4, 97 * DAY),
        waiting(8, 95 * DAY),
        Pull {
            bot: true,
            title: "Bump serde from 1.0.0 to 1.0.1".to_owned(),
            ..waiting(5, 95 * DAY)
        },
    ];
    let shown = board(&View::default(), &pulls, now());
    let nudged: Vec<(u64, String)> = nudges(&shown, &stages(), 10)
        .into_iter()
        .map(|nudge| (nudge.row.pull.number, stage_name(nudge.stage)))
        .collect();
    assert_eq!(
        nudged,
        [
            (3, "1w".to_owned()),
            (5, "2d".to_owned()),
            (8, "2d".to_owned()),
            (2, "2d".to_owned()),
            (4, "2d".to_owned())
        ]
    );
    let first: Vec<u64> = nudges(&shown, &stages(), 2)
        .into_iter()
        .map(|nudge| nudge.row.pull.number)
        .collect();
    assert_eq!(first, [3, 5]);
}

#[test]
fn a_nudge_is_an_event_of_its_rule_pull_spell_and_stage_seen_as_its_repository() {
    let pulls = [waiting(9, 97 * DAY)];
    let shown = board(&View::default(), &pulls, now());
    let nudge = nudges(&shown, &stages(), 1)[0];
    let event = nudge_event("review", nudge).unwrap();
    assert_eq!(
        event.identity().as_str(),
        "rule/review/github.com/nightworksio/herald/pull/9/1970-04-08T00:00:00Z/2d"
    );
    assert_eq!(event.kind(), &Kind::NUDGE);
    assert_eq!(event.project(), "herald");
    assert_eq!(event.title(), "Add a queue");
    assert_eq!(event.visibility(), Some(Visibility::Public));
    assert_eq!(event.occurred(), Timestamp::from_second(99 * DAY).unwrap());
    assert_eq!(
        event.links().get(PULL_LINK).map(String::as_str),
        Some("https://github.com/nightworksio/herald/pull/9")
    );
    let text = |text: &str| Some(Attribute::Text(text.to_owned()));
    let read = |name: &str| event.attribute(name).cloned();
    assert_eq!(read(RULE), text("review"));
    assert_eq!(read(STAGE), text("2d"));
    assert_eq!(read(TURN), text("waiting-on-review"));
    assert_eq!(read(REASON), text("review-requested"));
    assert_eq!(read(WAITED), text("3d"));
    assert_eq!(read(SPELL), text("1970-04-08T00:00:00Z"));
    assert_eq!(read(REPOSITORY), text("nightworksio/herald"));
    assert_eq!(read(NUMBER), Some(Attribute::Number(9.into())));
    assert_eq!(read(AUTHOR), text("wessel"));
    assert_eq!(read(BOT), Some(Attribute::Flag(false)));
    assert_eq!(read(WAITING_ON), text("ana, org/core"));
    assert!(
        crate::KindSpec::shipped()[9]
            .check(event.attributes())
            .is_ok()
    );
}

#[test]
fn a_nudge_names_no_author_or_login_the_forge_did_not_and_keeps_a_private_repository_private() {
    let pulls = [Pull {
        author: None,
        reviewers: Vec::new(),
        visibility: Visibility::Internal,
        updated: Timestamp::MAX,
        ..waiting(9, 0)
    }];
    let shown = board(&View::default(), &pulls, Timestamp::MAX);
    let row = &shown.groups[0].rows[0];
    let event = nudge_event(
        "review",
        Nudge {
            row,
            stage: hours(48),
        },
    )
    .unwrap();
    assert_eq!(event.attribute(AUTHOR), None);
    assert_eq!(event.attribute(WAITING_ON), None);
    assert_eq!(event.visibility(), Some(Visibility::Internal));
    assert_eq!(event.occurred(), Timestamp::MAX);
    let projectless = crate::queue::Row {
        pull: Pull {
            project: " ".to_owned(),
            ..row.pull.clone()
        },
        ..row.clone()
    };
    let nudge = Nudge {
        row: &projectless,
        stage: hours(48),
    };
    assert_eq!(nudge_event("review", nudge), Err(Problem::NoProject));
}

#[test]
fn a_pull_request_in_several_areas_is_nudged_once_and_the_most_counts_pull_requests() {
    let areas = crate::queue::of_paths(&["api", "docs", "web"]);
    let pulls = [
        Pull {
            areas: areas.clone(),
            ..waiting(3, 90 * DAY)
        },
        Pull {
            areas,
            ..waiting(2, 97 * DAY)
        },
        waiting(4, 96 * DAY),
    ];
    let view = View {
        group_by: crate::queue::GroupBy::Area,
        ..View::default()
    };
    let shown = board(&view, &pulls, now());
    assert_eq!(shown.groups.len(), 4);
    let nudged = |most: usize| -> Vec<u64> {
        nudges(&shown, &stages(), most)
            .into_iter()
            .map(|nudge| nudge.row.pull.number)
            .collect()
    };
    assert_eq!(nudged(10), [3, 4, 2]);
    assert_eq!(nudged(2), [3, 4]);
}

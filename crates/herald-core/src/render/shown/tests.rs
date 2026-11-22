use jiff::Timestamp;

use super::*;
use crate::members::Visibility;
use crate::queue::tests::pull;
use crate::queue::{Board, Bots, Failure, Head, Overviews, Running, Unread, View, board};

fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .try_iter()
        .unwrap()
        .map(|key| key.to_string())
        .collect();
    keys.sort();
    keys
}

fn shown() -> Board {
    let now = Timestamp::from_second(30 * 24 * 60 * 60).unwrap();
    let bump = crate::queue::Pull {
        number: 2,
        bot: true,
        title: "Bump serde from 1.0.1 to 1.0.2".to_owned(),
        ..pull()
    };
    board(
        &View {
            bots: Bots::Batch,
            ..View::default()
        },
        &[pull(), bump],
        now,
    )
}

#[test]
fn a_view_shows_its_name_counts_groups_and_batches_alone() {
    let board = shown();
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    assert_eq!(
        keys(&value),
        [
            "batches",
            "counts",
            "failures",
            "groups",
            "name",
            "overviews",
            "stale",
            "total",
            "withheld_pulls"
        ]
    );
    assert_eq!(value.get_attr("name").unwrap().to_string(), "queue");
    assert_eq!(value.get_attr("total").unwrap().to_string(), "2");
    let counts = value.get_attr("counts").unwrap();
    assert_eq!(
        keys(&counts.get_item_by_index(0).unwrap()),
        ["count", "state"]
    );
    let group = value
        .get_attr("groups")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(keys(&group), ["name", "rows"]);
    let row = group
        .get_attr("rows")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(
        keys(&row),
        [
            "areas",
            "author",
            "bot",
            "decided",
            "draft",
            "labels",
            "number",
            "opened",
            "overlaps",
            "project",
            "reason",
            "repository",
            "reviewers",
            "stale",
            "state",
            "title",
            "updated",
            "url",
            "waited",
            "waited_seconds",
            "waiting_on"
        ]
    );
    let batch = value
        .get_attr("batches")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(keys(&batch), ["failing", "green", "package", "rows", "to"]);
    assert_eq!(batch.get_attr("package").unwrap().to_string(), "serde");
    assert_eq!(batch.get_attr("to").unwrap().to_string(), "1.0.2");
}

#[test]
fn a_wait_is_written_in_its_largest_whole_unit() {
    assert_eq!(waited(SignedDuration::from_secs(59)), "0m");
    assert_eq!(waited(SignedDuration::from_mins(59)), "59m");
    assert_eq!(waited(SignedDuration::from_mins(60)), "1h");
    assert_eq!(waited(SignedDuration::from_hours(23)), "23h");
    assert_eq!(waited(SignedDuration::from_hours(24)), "1d");
    assert_eq!(waited(SignedDuration::from_hours(24 * 15)), "15d");
}

#[test]
fn a_view_as_json_holds_the_keys_a_template_reads() {
    let board = shown();
    let json: serde_json::Value = serde_json::from_str(&crate::render::view_json(Showing {
        view: "queue",
        board: &board,
    }))
    .unwrap();
    let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "batches",
            "counts",
            "failures",
            "groups",
            "name",
            "overviews",
            "stale",
            "total",
            "withheld_pulls"
        ]
    );
    assert_eq!(json["groups"][0]["rows"][0]["number"], 7);
}

#[test]
fn a_row_shows_what_was_decided_of_it_by_slot() {
    let mut board = shown();
    crate::decide::decided(
        &mut board.groups[0].rows[0],
        crate::decide::Slot::Priority,
        ("urgency", "high"),
    );
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let decided = value
        .get_attr("groups")
        .unwrap()
        .get_item_by_index(0)
        .unwrap()
        .get_attr("rows")
        .unwrap()
        .get_item_by_index(0)
        .unwrap()
        .get_attr("decided")
        .unwrap();
    assert_eq!(decided.get_attr("priority").unwrap().to_string(), "high");
    assert_eq!(keys(&decided), ["priority"]);
}

#[test]
fn a_failing_check_shows_its_name_count_state_and_the_pull_requests_it_fails_on() {
    let mut board = shown();
    board.failures.push(Failure {
        check: "test".to_owned(),
        pulls: vec![("o/app".to_owned(), 7), ("o/app".to_owned(), 9)],
        broken: true,
        flips: 2,
        flaky: true,
    });
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let failure = value
        .get_attr("failures")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(
        keys(&failure),
        ["broken", "check", "count", "flaky", "flips", "pulls"]
    );
    let read: Vec<String> = ["check", "count", "broken", "flips", "flaky"]
        .iter()
        .map(|key| failure.get_attr(key).unwrap().to_string())
        .collect();
    assert_eq!(read, ["test", "2", "True", "2", "True"]);
    let pull = failure
        .get_attr("pulls")
        .unwrap()
        .get_item_by_index(1)
        .unwrap();
    assert_eq!(keys(&pull), ["number", "repository"]);
    assert_eq!(
        (
            pull.get_attr("repository").unwrap().to_string(),
            pull.get_attr("number").unwrap().to_string()
        ),
        ("o/app".to_owned(), "9".to_owned())
    );
}

fn ran(id: &str, (queued, second): (bool, i64)) -> Running {
    Running {
        repository: "o/app".to_owned(),
        visibility: Visibility::Public,
        id: id.to_owned(),
        url: format!("https://github.com/o/app/actions/runs/{id}"),
        sha: id.to_owned(),
        pull: true,
        queued,
        created: Timestamp::from_second(second).unwrap(),
    }
}

#[test]
fn a_view_shows_its_runs_queued_and_running_the_oldest_queued_and_those_superseded() {
    let mut board = shown();
    let unread = |value: &Value| value.get_attr("overviews").unwrap().get_attr("unread");
    let none = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let overviews = none.get_attr("overviews").unwrap();
    assert!(overviews.get_attr("capacity").unwrap().is_none());
    assert_eq!(unread(&none).unwrap().len(), Some(0));
    board.overviews = Overviews {
        capacity: Some(vec![
            ran("31", (true, 2_000)),
            ran("32", (true, 1_000)),
            ran("33", (false, 500)),
        ]),
        unread: vec![Unread {
            repository: "o/vault".to_owned(),
            visibility: Visibility::Private,
            reason: "its runs are unread".to_owned(),
        }],
        heads: ["31", "32"]
            .map(|sha| Head {
                repository: "o/app".to_owned(),
                sha: sha.to_owned(),
            })
            .to_vec(),
        ..Overviews::default()
    };
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let capacity = value
        .get_attr("overviews")
        .unwrap()
        .get_attr("capacity")
        .unwrap();
    assert_eq!(
        keys(&capacity),
        ["oldest_queued", "queued", "running", "superseded"]
    );
    let read: Vec<String> = ["queued", "running", "oldest_queued"]
        .iter()
        .map(|key| capacity.get_attr(key).unwrap().to_string())
        .collect();
    assert_eq!(
        read,
        [
            "2",
            "1",
            &Timestamp::from_second(1_000).unwrap().to_string()
        ]
    );
    let superseded = capacity
        .get_attr("superseded")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(
        keys(&superseded),
        ["created", "id", "repository", "sha", "url"]
    );
    let shown: Vec<String> = ["id", "sha", "repository", "url", "created"]
        .iter()
        .map(|key| superseded.get_attr(key).unwrap().to_string())
        .collect();
    assert_eq!(
        shown,
        [
            "33",
            "33",
            "o/app",
            "https://github.com/o/app/actions/runs/33",
            &Timestamp::from_second(500).unwrap().to_string()
        ]
    );
    let unread = unread(&value).unwrap().get_item_by_index(0).unwrap();
    assert_eq!(
        (
            unread.get_attr("repository").unwrap().to_string(),
            unread.get_attr("reason").unwrap().to_string()
        ),
        ("o/vault".to_owned(), "its runs are unread".to_owned())
    );
}

fn advised(surface: crate::queue::Surface) -> Board {
    let at = Timestamp::from_second(3 * 24 * 60 * 60).unwrap();
    let found = crate::queue::Advisories {
        open: vec![crate::queue::Advisory {
            repository: "o/app".to_owned(),
            tool: "secret-scanning".to_owned(),
            severity: None,
            summary: Some("Example Service Key".to_owned()),
            state: "open".to_owned(),
            url: "https://github.com/o/app/security/secret-scanning/4".to_owned(),
            opened: Timestamp::from_second(24 * 60 * 60).unwrap(),
        }],
        unread: vec![crate::queue::Unread {
            repository: "o/vault".to_owned(),
            visibility: crate::Visibility::Private,
            reason: "refused".to_owned(),
        }],
        more: false,
        at,
    };
    let mut board = shown();
    board.overviews.advisories = Some(crate::queue::Advised::to(&found, surface));
    board
}

#[test]
fn a_private_surface_reads_each_advisory_by_its_allowlist() {
    let board = advised(crate::queue::Surface::Route {
        private: true,
        recallable: true,
    });
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let overviews = value.get_attr("overviews").unwrap();
    assert_eq!(keys(&overviews), ["advisories", "capacity", "unread"]);
    let advisories = overviews.get_attr("advisories").unwrap();
    assert_eq!(keys(&advisories), ["count", "more", "open", "unread"]);
    assert_eq!(advisories.get_attr("count").unwrap().to_string(), "1");
    let advisory = advisories
        .get_attr("open")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(
        keys(&advisory),
        [
            "age",
            "opened",
            "repository",
            "severity",
            "state",
            "summary",
            "tool",
            "url"
        ]
    );
    assert_eq!(advisory.get_attr("age").unwrap().to_string(), "2d");
    let unread = advisories
        .get_attr("unread")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(unread.get_attr("reason").unwrap().to_string(), "refused");
}

#[test]
fn a_surface_that_is_not_private_reads_only_that_a_section_was_withheld() {
    let board = advised(crate::queue::Surface::Route {
        private: false,
        recallable: true,
    });
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let overviews = value.get_attr("overviews").unwrap();
    assert_eq!(keys(&overviews), ["capacity", "unread", "withheld"]);
    assert_eq!(
        overviews.get_attr("withheld").unwrap().to_string(),
        crate::queue::WITHHELD
    );
    let unadvised = shown();
    let plain = view_value(Showing {
        view: "queue",
        board: &unadvised,
    });
    assert_eq!(
        keys(&plain.get_attr("overviews").unwrap()),
        ["capacity", "unread"]
    );
}

#[test]
fn a_row_names_each_area_with_its_kind() {
    let now = Timestamp::from_second(30 * 24 * 60 * 60).unwrap();
    let owned = crate::queue::Pull {
        areas: vec![
            crate::queue::Area {
                name: "@alice".to_owned(),
                kind: crate::queue::AreaKind::Person,
            },
            crate::queue::Area {
                name: "web".to_owned(),
                kind: crate::queue::AreaKind::Paths,
            },
        ],
        ..pull()
    };
    let board = board(&View::default(), &[owned], now);
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(
        json["groups"][0]["rows"][0]["areas"],
        serde_json::json!([
            { "name": "@alice", "kind": "person" },
            { "name": "web", "kind": "paths" }
        ])
    );
}

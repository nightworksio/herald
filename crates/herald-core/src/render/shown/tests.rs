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
            "readiness",
            "stale",
            "total",
            "withheld_pulls"
        ]
    );
    assert!(value.get_attr("readiness").unwrap().is_none());
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
            "base",
            "bot",
            "decided",
            "draft",
            "head_branch",
            "labels",
            "milestone",
            "number",
            "opened",
            "overlaps",
            "project",
            "queue",
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
            "readiness",
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
    assert_eq!(
        keys(&overviews),
        ["advisories", "capacity", "header", "load", "unread"]
    );
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
    assert_eq!(
        keys(&overviews),
        ["capacity", "header", "load", "unread", "withheld"]
    );
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
        ["capacity", "header", "load", "unread"]
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

#[test]
fn a_row_names_its_queue_entry_and_why_it_left() {
    use crate::queue::{Dequeued, Left, QueueEntry, QueueState};
    let now = Timestamp::from_second(30 * 24 * 60 * 60).unwrap();
    let at = Timestamp::from_second(1_000).unwrap();
    let waiting = crate::queue::Pull {
        queue: Some(QueueEntry {
            position: Some(2),
            state: QueueState::Checking,
            since: Some(at),
            estimate: Some(600),
            left: None,
        }),
        ..pull()
    };
    let gone = crate::queue::Pull {
        number: 8,
        queue: Some(QueueEntry::leaving(
            None,
            Left {
                at,
                reason: Dequeued::ChecksFailed,
            },
        )),
        ..pull()
    };
    let board = board(&View::default(), &[waiting, gone, pull()], now);
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    let json = serde_json::to_value(&value).unwrap();
    let queues: Vec<&serde_json::Value> = json["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["rows"].as_array().unwrap())
        .map(|row| &row["queue"])
        .collect();
    assert!(queues.contains(&&serde_json::json!({
        "position": 2, "state": "checking", "since": "1970-01-01T00:16:40Z",
        "estimate_seconds": 600, "left": null
    })));
    assert!(queues.contains(&&serde_json::json!({
        "position": null, "state": "queued", "since": null, "estimate_seconds": null,
        "left": { "at": "1970-01-01T00:16:40Z", "reason": "checks-failed", "failure": true }
    })));
    assert!(queues.contains(&&serde_json::Value::Null));
}

#[test]
fn a_view_shows_its_load_by_team_and_area_with_its_allowlisted_keys() {
    use crate::queue::{Area, AreaKind, Loaded, Loads};
    let mut board = shown();
    let load = |board: &Board| {
        view_value(Showing {
            view: "queue",
            board,
        })
        .get_attr("overviews")
        .unwrap()
        .get_attr("load")
        .unwrap()
    };
    assert!(load(&board).is_none());
    let count = Loaded {
        requested: 4,
        reviewed: 2,
    };
    board.overviews.load = Some(Loads {
        teams: vec![("acme/platform".to_owned(), count)],
        areas: vec![(
            Area {
                name: "backend".to_owned(),
                kind: AreaKind::Paths,
            },
            count,
        )],
        other: Some(count),
        reviewed: true,
    });
    let shown = load(&board);
    assert_eq!(keys(&shown), ["areas", "other", "teams"]);
    let team = shown
        .get_attr("teams")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(keys(&team), ["name", "requested", "reviewed"]);
    let area = shown
        .get_attr("areas")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(keys(&area), ["kind", "name", "requested", "reviewed"]);
    let read: Vec<String> = [
        team.get_attr("name"),
        team.get_attr("requested"),
        team.get_attr("reviewed"),
        area.get_attr("name"),
        area.get_attr("kind"),
        shown.get_attr("other").unwrap().get_attr("reviewed"),
    ]
    .into_iter()
    .map(|value| value.unwrap().to_string())
    .collect();
    assert_eq!(read, ["acme/platform", "4", "2", "backend", "paths", "2"]);
    board.overviews.load = Some(Loads {
        other: None,
        reviewed: false,
        ..board.overviews.load.clone().unwrap()
    });
    let live = load(&board);
    assert!(live.get_attr("other").unwrap().is_none());
    let team = live
        .get_attr("teams")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert!(team.get_attr("reviewed").unwrap().is_none());
    assert_eq!(team.get_attr("requested").unwrap().to_string(), "4");
}

#[test]
fn a_row_names_its_milestone_and_its_branches() {
    let milestoned = crate::queue::Pull {
        base: Some("main".to_owned()),
        head_branch: Some("fix".to_owned()),
        milestone: Some("v2.0".to_owned()),
        ..pull()
    };
    let now = Timestamp::from_second(30 * 24 * 60 * 60).unwrap();
    let board = board(&View::default(), &[milestoned], now);
    let row = row_value(board.distinct_rows().first().unwrap());
    let named: Vec<String> = ["milestone", "base", "head_branch"]
        .into_iter()
        .map(|key| row.get_attr(key).unwrap().to_string())
        .collect();
    assert_eq!(named, ["v2.0", "main", "fix"]);
}

#[test]
fn a_release_verdict_shows_ready_each_hold_and_those_past_the_bound() {
    use crate::queue::{Blocker, Held, HeldBy, Holding, Judged, Readiness};
    let mut board = shown();
    board.readiness = Some(Readiness::default());
    let json = |board: &Board| -> serde_json::Value {
        serde_json::from_str(&crate::render::view_json(Showing {
            view: "release",
            board,
        }))
        .unwrap()
    };
    assert_eq!(
        json(&board)["readiness"],
        serde_json::json!({ "ready": true, "holds": [], "more": 0 })
    );
    board.readiness = Some(Readiness {
        holds: vec![
            Held::Shown(Holding {
                held_by: HeldBy::Blocked(Blocker::Failed),
                repository: "a/b".to_owned(),
                number: 4,
                url: "https://example.com/a/b/pull/4".to_owned(),
                named: vec!["test".to_owned()],
                more: 2,
            }),
            Held::Private,
        ],
        more: 5,
        judged: Judged::Whole,
    });
    assert_eq!(
        json(&board)["readiness"],
        serde_json::json!({
            "ready": false,
            "holds": [{
                "private": false,
                "reason": "failed",
                "repository": "a/b",
                "number": 4,
                "url": "https://example.com/a/b/pull/4",
                "named": ["test"],
                "more": 2,
            }, {
                "private": true,
                "reason": null,
                "repository": null,
                "number": null,
                "url": null,
                "named": [],
                "more": 0,
            }],
            "more": 5,
        })
    );
    board.readiness = Some(Readiness {
        judged: Judged::Unknown,
        ..Readiness::default()
    });
    assert_eq!(
        json(&board)["readiness"],
        serde_json::json!({ "ready": null, "holds": [], "more": 0 })
    );
}

#[test]
fn a_release_header_shows_each_milestone_and_each_unread_repository() {
    use crate::queue::{Header, OpenMilestone, Unread};
    let mut board = shown();
    let json = |board: &Board| -> serde_json::Value {
        serde_json::from_str(&crate::render::view_json(Showing {
            view: "release",
            board,
        }))
        .unwrap()
    };
    assert_eq!(json(&board)["overviews"]["header"], serde_json::Value::Null);
    board.overviews.header = Some(Header {
        milestones: vec![
            OpenMilestone {
                repository: "a/b".to_owned(),
                visibility: crate::Visibility::Public,
                title: "v2.0".to_owned(),
                url: Some("https://example.com/a/b/milestone/1".to_owned()),
                due: Some("2026-11-22".parse().unwrap()),
                overdue: true,
                counts: Some((2, 12)),
            },
            OpenMilestone {
                repository: "a/c".to_owned(),
                visibility: crate::Visibility::Public,
                title: "v2.1".to_owned(),
                url: None,
                due: None,
                overdue: false,
                counts: None,
            },
        ],
        unread: vec![Unread {
            repository: "a/d".to_owned(),
            visibility: crate::Visibility::Public,
            reason: "refused".to_owned(),
        }],
    });
    assert_eq!(
        json(&board)["overviews"]["header"],
        serde_json::json!({
            "milestones": [{
                "repository": "a/b", "title": "v2.0",
                "url": "https://example.com/a/b/milestone/1", "due": "2026-11-22",
                "overdue": true, "open": 2, "closed": 12,
            }, {
                "repository": "a/c", "title": "v2.1", "url": null, "due": null,
                "overdue": false, "open": null, "closed": null,
            }],
            "unread": [{ "repository": "a/d", "reason": "refused" }],
        })
    );
}

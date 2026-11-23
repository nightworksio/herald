//! A release's verdict and header as `herald queue` prints them: a
//! private repository's pull requests and milestones withheld unless said
//! private, the verdict told as the release's `withheld` says (ADR-0081,
//! ADR-0082).

use serde_json::json;

use super::queue::{WITHHELD_ROWS, listed};
use super::world::*;
use crate::*;

fn shipped(withheld: &str) -> Setup {
    setup(&format!(
        "{CONFIG}[views.ship]\nmost_pulls = 0\nrelease = {{ milestone = \"v2.*\"{withheld} }}\n"
    ))
}

fn readiness_read(setup: &Setup, world: &World, private: bool) -> serde_json::Value {
    let mut args = vec!["queue", "ship", "--format", "json"];
    if private {
        args.push("--private");
    }
    let json = herald(setup, world, &args);
    assert_eq!(private, json.out.contains("cli/cli"), "{}", json.out);
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    read["readiness"].clone()
}

#[test]
fn a_release_some_of_whose_pull_requests_are_withheld_is_not_known_unless_said_private() {
    let world = World::default();
    listed(&world);
    for setup in [shipped(""), shipped(", withheld = \"unknown\"")] {
        let table = herald(&setup, &world, &["queue", "ship"]);
        assert_eq!((table.code, table.err.as_str()), (DONE, ""));
        assert_eq!(
            table.out.lines().collect::<Vec<_>>(),
            [
                "ship: 0 open pull requests, 0 stale",
                "release: not known, some pull requests are private",
                WITHHELD_ROWS
            ]
        );
        assert_eq!(
            readiness_read(&setup, &world, false),
            json!({ "ready": null, "holds": [], "more": 0 })
        );
        let printed = herald(&setup, &world, &["queue", "ship", "--private"]);
        assert_eq!(
            printed.out.lines().take(3).collect::<Vec<_>>(),
            [
                "ship: 1 open pull requests, 1 stale",
                "release: not ready, 1 reason",
                "  cli/cli#7 was not read in full",
            ]
        );
        let read = readiness_read(&setup, &world, true);
        assert_eq!(read["ready"], false);
        assert_eq!(read["holds"][0]["private"], false);
        assert_eq!(read["holds"][0]["reason"], "unread");
        assert_eq!(read["holds"][0]["url"], "https://github.com/cli/cli/pull/7");
    }
}

#[test]
fn a_release_judged_by_the_rows_printed_says_so_and_nothing_of_a_private_hold() {
    let setup = shipped(", withheld = \"shown\"");
    let world = World::default();
    listed(&world);
    let table = herald(&setup, &world, &["queue", "ship"]);
    assert_eq!(
        table.out.lines().collect::<Vec<_>>(),
        [
            "ship: 0 open pull requests, 0 stale",
            "release: ready, by the pull requests printed",
            WITHHELD_ROWS
        ]
    );
    assert_eq!(
        readiness_read(&setup, &world, false),
        json!({ "ready": true, "holds": [], "more": 0 })
    );
    let printed = herald(&setup, &world, &["queue", "ship", "--private"]);
    assert_eq!(
        printed.out.lines().nth(1),
        Some("release: not ready, 1 reason")
    );
    assert_eq!(readiness_read(&setup, &world, true)["ready"], false);
}

#[test]
fn a_masked_release_names_a_private_hold_private_and_nothing_more_unless_said_private() {
    let setup = shipped(", withheld = \"masked\"");
    let world = World::default();
    listed(&world);
    let table = herald(&setup, &world, &["queue", "ship"]);
    assert_eq!(
        table.out.lines().collect::<Vec<_>>(),
        [
            "ship: 0 open pull requests, 0 stale",
            "release: not ready, 1 reason",
            "  (private)",
            WITHHELD_ROWS
        ]
    );
    assert_eq!(
        readiness_read(&setup, &world, false),
        json!({
            "ready": false,
            "holds": [{
                "private": true,
                "reason": null,
                "repository": null,
                "number": null,
                "url": null,
                "named": [],
                "more": 0,
            }],
            "more": 0,
        })
    );
    let printed = herald(&setup, &world, &["queue", "ship", "--private"]);
    assert_eq!(
        printed.out.lines().nth(2),
        Some("  cli/cli#7 was not read in full")
    );
    let read = readiness_read(&setup, &world, true);
    assert_eq!(read["holds"][0]["reason"], "unread");
}

#[test]
fn a_release_verdict_names_each_hold_its_labels_or_checks_and_those_past_the_bound() {
    use herald_core::{Blocker, Held, HeldBy, Holding, Judged, Readiness};
    let holding = |held_by: HeldBy, named: &[&str], more: usize| {
        Held::Shown(Holding {
            held_by,
            repository: "a/b".to_owned(),
            number: 3,
            url: String::new(),
            named: named.iter().map(|name| (*name).to_owned()).collect(),
            more,
        })
    };
    let readiness = Readiness {
        holds: vec![
            holding(HeldBy::Blocked(Blocker::Label), &["blocker"], 0),
            holding(HeldBy::Blocked(Blocker::Failed), &["test", "lint"], 2),
            holding(HeldBy::Blocked(Blocker::Draft), &[], 0),
            holding(HeldBy::Blocked(Blocker::Changes), &[], 0),
            Held::Private,
        ],
        more: 6,
        judged: Judged::Whole,
    };
    assert_eq!(
        crate::queueing::judged(&readiness),
        [
            "release: not ready, 11 reasons",
            "  a/b#3 carries blocker",
            "  a/b#3 failed test, lint and 2 more",
            "  a/b#3 is a draft",
            "  a/b#3 has changes requested",
            "  (private)",
            "  and 6 more",
        ]
    );
    assert_eq!(
        crate::queueing::judged(&Readiness::default()),
        ["release: ready"]
    );
    let by_shown = Readiness {
        judged: Judged::Shown,
        ..Readiness::default()
    };
    assert_eq!(
        crate::queueing::judged(&by_shown),
        ["release: ready, by the pull requests printed"]
    );
    let unknown = Readiness {
        judged: Judged::Unknown,
        ..Readiness::default()
    };
    assert_eq!(
        crate::queueing::judged(&unknown),
        ["release: not known, some pull requests are private"]
    );
    let alone = Readiness {
        holds: vec![holding(HeldBy::Unread, &[], 0)],
        more: 0,
        judged: Judged::Shown,
    };
    assert_eq!(
        crate::queueing::judged(&alone),
        [
            "release: not ready, 1 reason",
            "  a/b#3 was not read in full"
        ]
    );
    let one_more = Readiness {
        holds: vec![holding(HeldBy::Unread, &[], 0)],
        more: 1,
        judged: Judged::Whole,
    };
    assert_eq!(
        crate::queueing::judged(&one_more),
        [
            "release: not ready, 2 reasons",
            "  a/b#3 was not read in full",
            "  and 1 more",
        ]
    );
}

#[test]
fn a_release_s_header_of_a_private_repository_is_printed_only_with_private() {
    let setup = shipped("");
    let world = World::default();
    listed(&world);
    let milestones = json!([{
        "id": 1, "number": 1, "title": "v2.0", "description": null, "state": "open",
        "open_issues": 1, "closed_issues": 3, "created_at": "2026-10-01T08:00:00Z",
        "updated_at": "2026-10-02T08:00:00Z", "closed_at": null, "due_on": "2026-10-01T08:00:00Z",
        "html_url": "https://github.com/cli/cli/milestone/1",
    }, {
        "id": 2, "number": 2, "title": "v2.1", "description": null, "state": "open",
        "open_issues": 0, "closed_issues": 0, "created_at": "2026-10-01T08:00:00Z",
        "updated_at": "2026-10-02T08:00:00Z", "closed_at": null, "due_on": "2099-01-01T08:00:00Z",
        "html_url": "https://github.com/cli/cli/milestone/2",
    }]);
    world.state.lock().unwrap().forge.insert(
        "/repos/cli/cli/milestones?state=open&per_page=30".to_owned(),
        (200, milestones.to_string()),
    );
    let table = herald(&setup, &world, &["queue", "ship"]);
    assert!(!table.out.contains("milestone"), "{}", table.out);
    let json = herald(&setup, &world, &["queue", "ship", "--format", "json"]);
    assert!(!json.out.contains("v2.0"), "{}", json.out);
    let printed = herald(&setup, &world, &["queue", "ship", "--private"]);
    assert_eq!(
        printed.out.lines().nth(3),
        Some(
            "  milestone v2.0 of cli/cli, due 2026-10-01, overdue, 1 open, 3 closed  https://github.com/cli/cli/milestone/1"
        ),
        "{}",
        printed.out
    );
    assert_eq!(
        printed.out.lines().nth(4),
        Some(
            "  milestone v2.1 of cli/cli, due 2099-01-01, 0 open, 0 closed  https://github.com/cli/cli/milestone/2"
        ),
        "{}",
        printed.out
    );
    world.state.lock().unwrap().forge.insert(
        "/repos/cli/cli/milestones?state=open&per_page=30".to_owned(),
        (403, "{}".to_owned()),
    );
    let refused = herald(&setup, &world, &["queue", "ship", "--private"]);
    assert!(
        refused.out.contains(
            "  milestones of cli/cli unread, because the token may not read its milestones"
        ),
        "{}",
        refused.out
    );
}

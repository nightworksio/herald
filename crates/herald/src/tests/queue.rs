use serde_json::json;

use super::world::*;
use crate::*;

const LISTED: &str =
    "/repos/cli/cli/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1";

fn viewed() -> String {
    format!(
        "{CONFIG}[views.mine]\nmost_pulls = 0\n\n[views.flat]\nmost_pulls = 0\ngroup_by = \"nothing\"\nbumps = [\"chore: lift {{package}}\"]\n"
    )
}

fn listed(world: &World) {
    let pull = |number: u64, title: &str, login: &str| {
        json!({
            "number": number, "title": title,
            "html_url": format!("https://github.com/cli/cli/pull/{number}"),
            "created_at": "2026-10-01T12:00:00Z", "updated_at": "2026-10-01T12:00:00Z",
            "user": { "login": login, "type": "User" }
        })
    };
    let pulls = json!([
        pull(7, "Add a queue", "wessel"),
        pull(8, "Bump serde from 1.0.0 to 1.0.1", "dependabot[bot]"),
        pull(9, "chore: lift tokio", "renovate[bot]"),
        pull(10, "Tidy things", "helper[bot]"),
    ]);
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(LISTED.to_owned(), (200, pulls.to_string()));
}

#[test]
fn a_view_is_printed_as_a_table_or_as_json() {
    let setup = setup(&viewed());
    let world = World::default();
    listed(&world);
    let table = herald(&setup, &world, &["queue", "mine"]);
    assert_eq!((table.code, table.err.as_str()), (DONE, ""));
    let lines: Vec<&str> = table.out.lines().collect();
    assert_eq!(lines[0], "mine: 4 open pull requests, 4 stale");
    assert_eq!(lines[1], "unknown (1)");
    assert!(
        lines[2].starts_with("  cli/cli#7  unknown: unread, waited ")
            && lines[2].ends_with(", stale  Add a queue"),
        "{}",
        lines[2]
    );
    let bumps: Vec<&str> = lines[3..]
        .iter()
        .copied()
        .filter(|line| !line.starts_with("  "))
        .collect();
    assert_eq!(
        bumps,
        [
            "bumps no pattern reads (2, 0 green, 0 failing)",
            "serde to 1.0.1 (1, 0 green, 0 failing)",
        ]
    );
    let flat = herald(&setup, &world, &["queue", "flat"]);
    let flat: Vec<&str> = flat
        .out
        .lines()
        .filter(|line| !line.starts_with("  "))
        .collect();
    assert_eq!(
        flat,
        [
            "flat: 4 open pull requests, 4 stale",
            "everyone (1)",
            "bumps no pattern reads (1, 0 green, 0 failing)",
            "serde to 1.0.1 (1, 0 green, 0 failing)",
            "tokio (1, 0 green, 0 failing)"
        ]
    );
    let json = herald(&setup, &world, &["queue", "mine", "--format", "json"]);
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(read["name"], "mine");
    assert_eq!(read["groups"][0]["rows"][0]["number"], 7);
}

#[test]
fn a_view_the_config_does_not_name_is_refused_and_the_views_named() {
    let setup = setup(&viewed());
    let world = World::default();
    let refused = herald(&setup, &world, &["queue", "missing"]);
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .err
            .contains("no view is named \"missing\"; the views are \"flat\", \"mine\", \"queue\""),
        "{}",
        refused.err
    );
}

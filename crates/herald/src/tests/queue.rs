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
    let table = herald(&setup, &world, &["queue", "mine", "--private"]);
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
    let flat = herald(&setup, &world, &["queue", "flat", "--private"]);
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
    let json = herald(
        &setup,
        &world,
        &["queue", "mine", "--format", "json", "--private"],
    );
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

#[test]
fn a_view_s_advisories_are_printed_only_where_the_operator_says_the_output_is_private() {
    let setup = setup(&format!(
        "{CONFIG}[views.security]\nmost_pulls = 0\noverviews = [\"advisories\"]\n"
    ));
    let world = World::default();
    listed(&world);
    let alert = json!([{
        "number": 3, "state": "open",
        "html_url": "https://github.com/cli/cli/security/dependabot/3",
        "created_at": "2026-10-01T12:00:00Z",
        "security_advisory": { "summary": "Prototype pollution", "severity": "high" },
    }]);
    let mut state = world.state.lock().unwrap();
    for (listing, answer) in [
        ("code-scanning", (200, "[]".to_owned())),
        ("dependabot", (200, alert.to_string())),
        ("secret-scanning", (403, "{}".to_owned())),
    ] {
        let hidden = if listing == "secret-scanning" {
            "&hide_secret=true"
        } else {
            ""
        };
        let path = format!("/repos/cli/cli/{listing}/alerts?state=open&per_page=100{hidden}");
        state.forge.insert(path, answer);
    }
    drop(state);
    let withheld = herald(&setup, &world, &["queue", "security"]);
    assert_eq!((withheld.code, withheld.err.as_str()), (DONE, ""));
    assert_eq!(withheld.out.lines().last(), Some(herald_core::WITHHELD));
    assert!(!withheld.out.contains("dependabot"), "{}", withheld.out);
    let json = herald(&setup, &world, &["queue", "security", "--format", "json"]);
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(read["overviews"]["withheld"], herald_core::WITHHELD);
    assert!(read["overviews"].get("advisories").is_none());
    let private = herald(&setup, &world, &["queue", "security", "--private"]);
    let lines: Vec<&str> = private.out.lines().collect();
    assert!(lines.contains(&"advisories (1)"), "{}", private.out);
    assert!(
        lines.iter().any(|line| line
            .starts_with("  cli/cli dependency-scanning high: Prototype pollution, open, ")
            && line.ends_with("  https://github.com/cli/cli/security/dependabot/3")),
        "{}",
        private.out
    );
    assert!(
        lines.iter().any(|line| line.starts_with(
            "  cli/cli unread, because GitHub's secret scanning alerts: the token may not read them"
        )),
        "{}",
        private.out
    );
    assert!(!private.out.contains(herald_core::WITHHELD));
    let json = herald(
        &setup,
        &world,
        &["queue", "security", "--format", "json", "--private"],
    );
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(read["overviews"]["advisories"]["count"], 1);
    assert!(read["overviews"].get("withheld").is_none());
}

const WITHHELD_ROWS: &str = "4 pull requests of private or internal repositories withheld: --private, or [cli] private, prints them";

#[test]
fn a_private_repository_s_rows_are_withheld_and_counted_unless_the_output_is_said_private() {
    let setup = setup(&viewed());
    let world = World::default();
    listed(&world);
    let table = herald(&setup, &world, &["queue", "mine"]);
    assert_eq!((table.code, table.err.as_str()), (DONE, ""));
    assert_eq!(
        table.out.lines().collect::<Vec<_>>(),
        ["mine: 0 open pull requests, 0 stale", WITHHELD_ROWS]
    );
    let json = herald(&setup, &world, &["queue", "mine", "--format", "json"]);
    assert!(!json.out.contains("cli/cli"), "{}", json.out);
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(read["withheld_pulls"], 4);
    assert_eq!(read["total"], 0);
    for asked in [
        &["queue", "mine", "--private"][..],
        &["queue", "mine", "--format", "json", "--private"][..],
    ] {
        let printed = herald(&setup, &world, asked);
        assert!(printed.out.contains("cli/cli"), "{}", printed.out);
        assert!(!printed.out.contains(WITHHELD_ROWS), "{}", printed.out);
    }
    let read: serde_json::Value = serde_json::from_str(
        &herald(
            &setup,
            &world,
            &["queue", "mine", "--format", "json", "--private"],
        )
        .out,
    )
    .unwrap();
    assert_eq!(
        (read["withheld_pulls"].clone(), read["total"].clone()),
        (json!(0), json!(4))
    );
    let configured = super::world::setup(&format!("{}[cli]\nprivate = true\n", viewed()));
    for format in ["table", "json"] {
        let printed = herald(&configured, &world, &["queue", "mine", "--format", format]);
        assert!(
            printed.out.contains("cli/cli#7") || printed.out.contains("\"number\": 7"),
            "{}",
            printed.out
        );
        assert!(!printed.out.contains(WITHHELD_ROWS), "{}", printed.out);
    }
}

#[test]
fn the_cli_setting_prints_private_rows_but_never_a_view_s_advisories() {
    let setup = setup(&format!(
        "{CONFIG}[views.security]\nmost_pulls = 0\noverviews = [\"advisories\"]\n\n[cli]\nprivate = true\n"
    ));
    let world = World::default();
    listed(&world);
    let table = herald(&setup, &world, &["queue", "security"]);
    assert!(table.out.contains("cli/cli#7"), "{}", table.out);
    assert_eq!(table.out.lines().last(), Some(herald_core::WITHHELD));
    assert!(!table.out.contains("advisories ("), "{}", table.out);
    let json = herald(&setup, &world, &["queue", "security", "--format", "json"]);
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(read["overviews"]["withheld"], herald_core::WITHHELD);
    assert!(read["overviews"].get("advisories").is_none());
    assert!(!json.out.contains("security/dependabot"), "{}", json.out);
    assert!(
        world
            .state
            .lock()
            .unwrap()
            .seen
            .iter()
            .all(|asked| !asked.contains("/alerts")),
        "no advisory is read where none may be printed"
    );
}

#[test]
fn clearing_a_report_s_orphans_where_it_holds_none_is_refused() {
    let setup = setup(&viewed());
    let world = World::default();
    let refused = herald(&setup, &world, &["orphans", "clear", "weekly"]);
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .err
            .contains("no living post of the report weekly holds a possibly orphaned part"),
        "{}",
        refused.err
    );
}

/// The ledger key of the report `live`'s living post on `route`.
fn living(route: &str) -> herald_core::Key {
    herald_core::Key::derive(
        &herald_core::RouteId::new(route),
        &herald_core::Identity::new(herald_core::living_identity("live")),
    )
}

/// The report `live`'s living posts on the routes `a` and `b`, kept in
/// the store `world` holds, each with one part sent and never found again.
fn strayed(world: &World) {
    strayed_parts(world, &[0]);
}

/// The report `live`'s living posts on the routes `a` and `b`, kept in
/// the store `world` holds, each with `parts` sent and never found again.
fn strayed_parts(world: &World, parts: &[u32]) {
    use herald_ports::{Cursors as _, Ledger as _, Store as _};
    let store = kept(world);
    let posts: Vec<herald_core::AdvisedPost> = ["a", "b"]
        .into_iter()
        .map(|route| herald_core::AdvisedPost {
            route: herald_core::RouteId::new(route),
            destination: herald_core::DestinationName::new("room"),
            placed: String::new(),
        })
        .collect();
    let cursor = herald_core::Cursor {
        advised: posts,
        ..herald_core::Cursor::default()
    };
    let live = herald_core::SourceName::new("live");
    pollster::block_on(store.cursors().put_cursor(&live, &cursor, None)).unwrap();
    for route in ["a", "b"] {
        let mut entry = herald_core::Entry::new(living(route));
        for (sent, part) in parts.iter().enumerate() {
            entry.strays.push(herald_core::Stray {
                part: *part,
                token: format!("{route}.{sent}"),
            });
        }
        pollster::block_on(store.ledger().put_entry(&entry, None)).unwrap();
    }
}

/// How many parts of the report `live`'s living post on `route` the store
/// `world` holds still keeps as possibly orphaned.
fn strays(world: &World, route: &str) -> usize {
    use herald_ports::{Ledger as _, Store as _};
    let store = kept(world);
    pollster::block_on(store.ledger().entry(&living(route)))
        .unwrap()
        .map_or(0, |entry| entry.value.strays.len())
}

#[test]
fn orphans_are_listed_with_each_route_s_digest_and_cleared_one_route_at_a_time() {
    let setup = setup(&viewed());
    let world = World::default();
    strayed(&world);
    let listed = herald(&setup, &world, &["orphans", "clear", "live"]);
    assert_eq!((listed.code, listed.err.as_str()), (DONE, ""));
    let lines: Vec<&str> = listed.out.lines().collect();
    assert_eq!(lines.len(), 5, "{}", listed.out);
    let seen = |route: &str| {
        let confirm = format!(
            "  once deleted by hand: herald orphans clear live --route {route} --deleted --seen "
        );
        let line = lines
            .iter()
            .find_map(|line| line.strip_prefix(confirm.as_str()))
            .unwrap_or_else(|| panic!("{}", listed.out));
        assert!(
            line.len() == 12 && line.chars().all(|digit| digit.is_ascii_hexdigit()),
            "{line}"
        );
        line.to_owned()
    };
    assert_eq!(
        [lines[0], lines[2]],
        [
            "possibly orphaned: the post of the report live's living post on the route a",
            "possibly orphaned: the post of the report live's living post on the route b",
        ]
    );
    let (a, b) = (seen("a"), seen("b"));
    assert_ne!(a, b);
    assert!(
        lines[4].starts_with(
            "Clearing a route's parts with the command shown makes herald stop tracking them"
        ),
        "{}",
        lines[4]
    );
    assert_eq!((strays(&world, "a"), strays(&world, "b")), (1, 1));
    let unnamed = herald(&setup, &world, &["orphans", "clear", "live", "--deleted"]);
    assert_ne!(unnamed.code, DONE);
    assert_eq!((strays(&world, "a"), strays(&world, "b")), (1, 1));
    let confirm = [
        "orphans",
        "clear",
        "live",
        "--route",
        "a",
        "--deleted",
        "--seen",
        a.as_str(),
    ];
    let cleared = herald(&setup, &world, &confirm);
    assert_eq!((cleared.code, cleared.err.as_str()), (DONE, ""));
    assert_eq!(
        cleared.out.trim_end(),
        "cleared: the post of the report live's living post on the route a"
    );
    assert_eq!((strays(&world, "a"), strays(&world, "b")), (0, 1));
    let again = herald(&setup, &world, &confirm);
    assert_eq!(again.code, STOPPED);
    assert!(
        again
            .err
            .contains("no living post of the report live holds a possibly orphaned part on the route a, so nothing was cleared"),
        "{}",
        again.err
    );
    assert_eq!(strays(&world, "b"), 1);
}

#[test]
fn a_part_sent_more_than_once_is_listed_once_with_how_many_times() {
    let setup = setup(&viewed());
    let world = World::default();
    strayed_parts(&world, &[2, 0, 0]);
    let listed = herald(&setup, &world, &["orphans", "clear", "live"]);
    assert_eq!((listed.code, listed.err.as_str()), (DONE, ""));
    assert_eq!(
        listed.out.lines().next(),
        Some(
            "possibly orphaned: the post (sent 2 times), chunk 2 of the report live's living post on the route a"
        ),
        "{}",
        listed.out
    );
}

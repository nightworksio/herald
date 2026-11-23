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
    let mut pulls = json!([
        pull(7, "Add a queue", "wessel"),
        pull(8, "Bump serde from 1.0.0 to 1.0.1", "dependabot[bot]"),
        pull(9, "chore: lift tokio", "renovate[bot]"),
        pull(10, "Tidy things", "helper[bot]"),
    ]);
    pulls[0]["milestone"] = json!({ "title": "v2.0" });
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
            && lines[2].ends_with(", stale, milestone v2.0  Add a queue"),
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

/// What of the private repository `acme/secret-plans` no withheld output
/// may hold: its owner, its name, its pull requests' numbers and titles,
/// and their heads.
const SECRET_NAMED: [&str; 7] = [
    "acme",
    "merger-launch",
    "secret-plans",
    "7041",
    "7042",
    "Merger plan",
    "c0ffee",
];

/// The merge queue's estimate for `secret_kept`'s first pull request, in
/// seconds, which no withheld output prints.
const SECRET_ESTIMATE: u64 = 7_777;

/// The path both of `secret_kept`'s pull requests change, which no output
/// prints.
const SECRET_PATH: &str = "plans/merger/launch.md";

/// `secret_kept`'s first pull request's entry in the merge queue, third
/// since `at`, and its second's, which left it for a failed check at `at`.
fn secret_queues(at: jiff::Timestamp) -> [herald_core::QueueEntry; 2] {
    let entry = herald_core::QueueEntry {
        position: Some(3),
        state: herald_core::QueueState::Checking,
        since: Some(at),
        estimate: Some(SECRET_ESTIMATE),
        left: None,
    };
    let left = herald_core::Left {
        at,
        reason: herald_core::Dequeued::ChecksFailed,
    };
    let gone = herald_core::QueueEntry::leaving(Some(&entry), left);
    [entry, gone]
}

/// `world`'s store keeping two private pull requests of
/// `acme/secret-plans` that change one path, the first's check `test`
/// failing on its head and having failed then passed on it an hour ago,
/// the first third in the merge queue and the second having left it for a
/// failed check, read now; and its CI runs, a full page queued, one running on a head no
/// pull request is on, and the failed run 42 on the first head, whose job
/// `test` failed.
fn secret_kept(world: &World) {
    use herald_core::{Changed, Checks, Flip, KeptPull, PullKey, Reconciled, RepositoryKey};
    use herald_ports::{Pulls as _, Store as _};
    let now = jiff::Timestamp::now();
    let ago = now - jiff::SignedDuration::from_hours(1);
    let key = RepositoryKey {
        host: "github.com".to_owned(),
        id: "7".to_owned(),
    };
    let store = kept(world);
    let pulls = store.pulls().unwrap();
    let [waiting, gone] = secret_queues(ago);
    for (number, flipped) in [(7041, true), (7042, false)] {
        let queue = if flipped { &waiting } else { &gone }.clone();
        let head = format!("c0ffee{number}");
        let (checks, flips) = if flipped {
            let flip = Flip {
                check: "test".to_owned(),
                head: head.clone(),
                at: ago,
            };
            (Checks::Failed(vec!["test".to_owned()]), vec![flip])
        } else {
            (Checks::Passed, Vec::new())
        };
        let pull = herald_core::Pull {
            project: "cli".to_owned(),
            repository: "acme/secret-plans".to_owned(),
            visibility: herald_core::Visibility::Private,
            number,
            title: format!("Merger plan, part {number}"),
            url: format!("https://github.com/acme/secret-plans/pull/{number}"),
            author: Some("ana".to_owned()),
            bot: false,
            draft: false,
            labels: Vec::new(),
            opened: ago,
            updated: ago,
            review: herald_core::Review::Approved,
            reviewers: Vec::new(),
            checks,
            mergeable: herald_core::Mergeable::Clean,
            queue: Some(queue),
            forge: None,
            head: Some(head),
            unread: false,
            base: Some("main".to_owned()),
            head_branch: None,
            milestone: None,
            files: Some(Changed {
                paths: [SECRET_PATH.to_owned()].into(),
                whole: true,
            }),
            areas: Vec::new(),
            flips,
        };
        let pulled = PullKey {
            repository: key.clone(),
            number,
        };
        let record = KeptPull::read(None, pulled, pull, now);
        pollster::block_on(pulls.put_pull(&record, None)).unwrap();
    }
    let mark = Reconciled {
        repository: Some(key),
        at: Some(now),
        ..Reconciled::default()
    };
    pollster::block_on(pulls.put_reconciled(&herald_core::SourceName::new("cli"), &mark, None))
        .unwrap();
    let runs = |listed: &[(u64, &str)]| {
        let runs: Vec<_> = listed
            .iter()
            .map(|(id, sha)| {
                json!({ "id": id, "html_url": format!("https://github.com/acme/secret-plans/actions/runs/{id}"),
                    "head_sha": sha, "event": "pull_request", "created_at": "2026-11-20T10:00:00Z" })
            })
            .collect();
        json!({ "workflow_runs": runs }).to_string()
    };
    let full: Vec<(u64, &str)> = (1..=100).map(|id| (id, "c0ffee7041")).collect();
    let forge = &mut world.state.lock().unwrap().forge;
    let jobs = json!({ "jobs": [{ "id": 420, "name": "test", "conclusion": "failure" }] });
    for (asked, listed) in [
        ("?status=queued&per_page=100", runs(&full)),
        (
            "?status=in_progress&per_page=100",
            runs(&[(101, "c0ffee7040")]),
        ),
        (
            "?head_sha=c0ffee7041&status=failure&per_page=5",
            runs(&[(42, "c0ffee7041")]),
        ),
        ("/42/jobs?filter=latest&per_page=100", jobs.to_string()),
    ] {
        forge.insert(
            format!("/repos/acme/secret-plans/actions/runs{asked}"),
            (200, listed),
        );
    }
}

#[test]
fn a_private_repository_s_areas_conflicts_flaky_check_runs_and_unread_line_are_printed_nowhere_unless_said_private()
 {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"acme/secret-plans\"\n",
    );
    let setup = setup(&format!(
        "{config}[sources.cli.areas]\nmerger-launch = [\"plans/**\"]\n[views.kept]\nsources = [\"cli\"]\nmode = \"kept\"\nreconcile = \"1d\"\nconflicts = true\noverviews = [\"capacity\"]\n[rules.flaky]\nview = \"kept\"\nflaky = {{ rerun = true }}\nevery = \"1h\"\nat = \"00:00\"\n"
    ));
    let world = World::default();
    secret_kept(&world);
    let printed = |args: &[&str]| {
        let run = herald(&setup, &world, args);
        assert_eq!(run.code, DONE, "{}", run.err);
        format!("{}{}", run.out, run.err)
    };
    let json = printed(&["queue", "kept", "--format", "json", "--private"]);
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        read["groups"][0]["rows"][0]["overlaps"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        read["groups"][0]["rows"][0]["areas"],
        json!([{ "name": "merger-launch", "kind": "paths" }])
    );
    assert_eq!(read["failures"][0]["pulls"][0]["number"], 7041);
    assert_eq!(read["overviews"]["capacity"]["queued"], 100);
    assert_eq!(
        read["overviews"]["unread"][0]["repository"],
        "acme/secret-plans"
    );
    let said = format!(
        "{json}{}{}",
        printed(&["rerun", "flaky", "--private"]),
        printed(&["queue", "kept", "--private"])
    );
    for named in SECRET_NAMED {
        assert!(said.contains(named), "{named}: {said}");
    }
    assert!(!said.contains(SECRET_PATH), "{said}");
    let table = printed(&["queue", "kept"]);
    assert_eq!(
        table.lines().collect::<Vec<_>>(),
        [
            "kept: 0 open pull requests, 0 stale",
            "2 pull requests of private or internal repositories withheld: --private, or [cli] private, prints them",
        ]
    );
    let json = printed(&["queue", "kept", "--format", "json"]);
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(read["withheld_pulls"], 2);
    let said = format!("{table}{json}{}", printed(&["rerun", "flaky"]));
    for named in SECRET_NAMED.iter().chain([&SECRET_PATH]) {
        assert!(!said.contains(named), "{named}: {said}");
    }
}

#[test]
fn a_private_repository_s_merge_queue_position_state_and_timing_are_printed_nowhere_unless_said_private()
 {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"acme/secret-plans\"\n",
    );
    let setup = setup(&format!(
        "{config}[views.kept]\nsources = [\"cli\"]\nmode = \"kept\"\nreconcile = \"1d\"\nqueue = true\n"
    ));
    let world = World::default();
    secret_kept(&world);
    let printed = |args: &[&str]| {
        let run = herald(&setup, &world, args);
        assert_eq!(run.code, DONE, "{}", run.err);
        format!("{}{}", run.out, run.err)
    };
    let table = printed(&["queue", "kept", "--private"]);
    assert!(
        table.contains(", checking in the merge queue at 3"),
        "{table}"
    );
    assert!(
        table.contains(", left the merge queue: checks-failed"),
        "{table}"
    );
    let json = printed(&["queue", "kept", "--format", "json", "--private"]);
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    let queues: Vec<&serde_json::Value> = read["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["rows"].as_array().unwrap())
        .map(|row| &row["queue"])
        .collect();
    assert!(queues.iter().any(|queue| queue["position"] == 3
        && queue["estimate_seconds"] == SECRET_ESTIMATE
        && queue["state"] == "checking"));
    assert!(
        queues
            .iter()
            .any(|queue| queue["left"]["reason"] == "checks-failed")
    );
    let said = format!(
        "{}{}",
        printed(&["queue", "kept"]),
        printed(&["queue", "kept", "--format", "json"])
    );
    let estimate = SECRET_ESTIMATE.to_string();
    for named in [
        "merge queue",
        "checking",
        "checks-failed",
        "estimate",
        "position",
        "left",
        estimate.as_str(),
    ]
    .iter()
    .chain(SECRET_NAMED.iter())
    {
        assert!(!said.contains(named), "{named}: {said}");
    }
}

/// Where `secret_kept`'s repository's CODEOWNERS file is read, at the
/// branch its pull requests merge into.
const SECRET_CODEOWNERS: &str = "/repos/acme/secret-plans/contents/.github/CODEOWNERS?ref=main";

/// The owners `secret_kept`'s repository's CODEOWNERS file names, which no
/// output prints unless it is said private.
const SECRET_OWNERS: [&str; 3] = ["merger-team", "ana-secret", "ana@example.com"];

/// The config reading `secret_kept`'s repository's areas from its
/// CODEOWNERS file, kept.
fn codeowned() -> String {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"acme/secret-plans\"\nareas = \"codeowners\"\n",
    );
    format!(
        "{config}[views.kept]\nsources = [\"cli\"]\nmode = \"kept\"\nreconcile = \"1d\"\ngroup_by = \"area\"\n"
    )
}

#[test]
fn a_private_repository_s_codeowners_areas_are_printed_nowhere_unless_said_private() {
    let setup = setup(&codeowned());
    let world = World::default();
    secret_kept(&world);
    world.state.lock().unwrap().raw.insert(
        SECRET_CODEOWNERS.to_owned(),
        "plans/ @acme/merger-team @ana-secret ana@example.com\n".to_owned(),
    );
    let printed = |args: &[&str]| {
        let run = herald(&setup, &world, args);
        assert_eq!(run.code, DONE, "{}", run.err);
        format!("{}{}", run.out, run.err)
    };
    let json = printed(&["queue", "kept", "--format", "json", "--private"]);
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        read["groups"][0]["rows"][0]["areas"],
        json!([
            { "name": "@acme/merger-team", "kind": "team" },
            { "name": "@ana-secret", "kind": "person" }
        ])
    );
    assert_eq!(read["total"], 2);
    assert!(!json.contains("ana@example.com"), "{json}");
    let table = printed(&["queue", "kept", "--private"]);
    assert!(
        table.contains("in @acme/merger-team, @ana-secret"),
        "{table}"
    );
    let said = format!(
        "{}{}",
        printed(&["queue", "kept"]),
        printed(&["queue", "kept", "--format", "json"])
    );
    for named in SECRET_OWNERS.iter().chain(&SECRET_NAMED) {
        assert!(!said.contains(named), "{named}: {said}");
    }
}

#[test]
fn a_codeowners_read_that_fails_names_only_its_source_unless_the_output_is_said_private() {
    let setup = setup(&codeowned());
    let world = World::default();
    secret_kept(&world);
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(SECRET_CODEOWNERS.to_owned(), (403, "{}".to_owned()));
    let short = herald(&setup, &world, &["queue", "kept"]);
    assert_eq!(short.code, STOPPED);
    assert_eq!(
        short.err.trim(),
        format!(
            "the source \"cli\" could not be read: its forge's answer could not be read; {}",
            herald_flows::UNSAID_FAILURE
        )
    );
    let full = herald(&setup, &world, &["queue", "kept", "--private"]);
    assert_eq!(full.code, STOPPED);
    assert!(full.err.contains("answered with 403"), "{}", full.err);
}

#[test]
fn a_read_that_fails_names_only_its_source_unless_the_output_is_said_private() {
    let setup = setup(&viewed());
    let world = World::default();
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(LISTED.to_owned(), (500, "{}".to_owned()));
    let short = herald(&setup, &world, &["queue", "mine"]);
    assert_eq!(short.code, STOPPED);
    assert_eq!(
        short.err.trim(),
        format!(
            "the source \"cli\" could not be read: its forge's answer could not be read; {}",
            herald_flows::UNSAID_FAILURE
        )
    );
    let full = herald(&setup, &world, &["queue", "mine", "--private"]);
    assert_eq!(full.code, STOPPED);
    assert!(full.err.contains("answered with 500"), "{}", full.err);
}

#[test]
fn a_release_verdict_is_judged_by_the_rows_printed_so_a_private_hold_is_printed_only_where_said_private()
 {
    let setup = setup(&format!(
        "{CONFIG}[views.ship]\nmost_pulls = 0\nrelease = {{ milestone = \"v2.*\" }}\n"
    ));
    let world = World::default();
    listed(&world);
    let table = herald(&setup, &world, &["queue", "ship"]);
    assert_eq!((table.code, table.err.as_str()), (DONE, ""));
    assert_eq!(
        table.out.lines().collect::<Vec<_>>(),
        [
            "ship: 0 open pull requests, 0 stale",
            "release: ready, by the pull requests printed",
            WITHHELD_ROWS
        ]
    );
    let json = herald(&setup, &world, &["queue", "ship", "--format", "json"]);
    assert!(!json.out.contains("cli/cli"), "{}", json.out);
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(
        read["readiness"],
        json!({ "ready": true, "holds": [], "more": 0 })
    );
    let printed = herald(&setup, &world, &["queue", "ship", "--private"]);
    let lines: Vec<&str> = printed.out.lines().collect();
    assert_eq!(
        lines[..3],
        [
            "ship: 1 open pull requests, 1 stale",
            "release: not ready, 1 reason",
            "  cli/cli#7 was not read in full",
        ]
    );
    let json = herald(
        &setup,
        &world,
        &["queue", "ship", "--format", "json", "--private"],
    );
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(read["readiness"]["ready"], false);
    assert_eq!(read["readiness"]["holds"][0]["reason"], "unread");
    assert_eq!(
        read["readiness"]["holds"][0]["url"],
        "https://github.com/cli/cli/pull/7"
    );
}

#[test]
fn a_release_verdict_names_each_hold_its_labels_or_checks_and_those_past_the_bound() {
    use herald_core::{Blocker, HeldBy, Holding, Readiness};
    let holding = |held_by: HeldBy, named: &[&str], more: usize| Holding {
        held_by,
        repository: "a/b".to_owned(),
        number: 3,
        url: String::new(),
        named: named.iter().map(|name| (*name).to_owned()).collect(),
        more,
    };
    let readiness = Readiness {
        holds: vec![
            holding(HeldBy::Blocked(Blocker::Label), &["blocker"], 0),
            holding(HeldBy::Blocked(Blocker::Failed), &["test", "lint"], 2),
            holding(HeldBy::Blocked(Blocker::Draft), &[], 0),
            holding(HeldBy::Blocked(Blocker::Changes), &[], 0),
        ],
        more: 6,
    };
    assert_eq!(
        crate::queueing::judged(&readiness, 0),
        [
            "release: not ready, 10 reasons",
            "  a/b#3 carries blocker",
            "  a/b#3 failed test, lint and 2 more",
            "  a/b#3 is a draft",
            "  a/b#3 has changes requested",
            "  and 6 more",
        ]
    );
    assert_eq!(
        crate::queueing::judged(&Readiness::default(), 0),
        ["release: ready"]
    );
    assert_eq!(
        crate::queueing::judged(&Readiness::default(), 1),
        ["release: ready, by the pull requests printed"]
    );
    let alone = Readiness {
        holds: vec![holding(HeldBy::Unread, &[], 0)],
        more: 0,
    };
    assert_eq!(
        crate::queueing::judged(&alone, 0),
        [
            "release: not ready, 1 reason",
            "  a/b#3 was not read in full"
        ]
    );
    let one_more = Readiness {
        holds: vec![holding(HeldBy::Unread, &[], 0)],
        more: 1,
    };
    assert_eq!(
        crate::queueing::judged(&one_more, 0),
        [
            "release: not ready, 2 reasons",
            "  a/b#3 was not read in full",
            "  and 1 more",
        ]
    );
}

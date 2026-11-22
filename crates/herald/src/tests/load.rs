use serde_json::json;

use super::world::*;
use crate::*;

/// Every name a private repository's load names, which no output prints
/// unless it is said private.
const SECRET_LOAD: [&str; 6] = [
    "merger-team",
    "duo-secret",
    "pair-secret",
    "merger-launch",
    "ana-secret",
    "bo-secret",
];

/// One open pull request of the private `acme/secret-plans`, kept: asked of
/// `acme/merger-team` and `ana-secret`, reviewed by `bo-secret` and
/// `cy-secret` within the window, changing a path in `merger-launch`.
fn secret_reviewed(world: &World) {
    use herald_core::{
        Changed, KeptPull, PullKey, Reconciled, RepositoryKey, Reviewed, Verdict, Visibility,
    };
    use herald_ports::{Pulls as _, Store as _};
    let now = jiff::Timestamp::now();
    let ago = now - jiff::SignedDuration::from_hours(1);
    let key = RepositoryKey {
        host: "github.com".to_owned(),
        id: "7".to_owned(),
    };
    let pull = herald_core::Pull {
        project: "cli".to_owned(),
        repository: "acme/secret-plans".to_owned(),
        visibility: Visibility::Private,
        number: 7041,
        title: "Merger plan".to_owned(),
        url: "https://github.com/acme/secret-plans/pull/7041".to_owned(),
        author: Some("ana".to_owned()),
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: ago,
        updated: ago,
        review: herald_core::Review::Requested,
        reviewers: vec!["acme/merger-team".to_owned(), "ana-secret".to_owned()],
        checks: herald_core::Checks::Passed,
        mergeable: herald_core::Mergeable::Clean,
        queued: false,
        forge: None,
        head: Some("c0ffee".to_owned()),
        unread: false,
        base: Some("main".to_owned()),
        files: Some(Changed {
            paths: ["plans/launch.md".to_owned()].into(),
            whole: true,
        }),
        areas: Vec::new(),
        flips: Vec::new(),
    };
    let pulled = PullKey {
        repository: key.clone(),
        number: 7041,
    };
    let mut record = KeptPull::read(None, pulled, pull, now);
    for login in ["bo-secret", "cy-secret"] {
        let reviewed = Reviewed {
            verdict: Verdict::Approved,
            decides: None,
            at: ago,
        };
        record.reviews.insert(login.to_owned(), reviewed);
    }
    let store = kept(world);
    let pulls = store.pulls().unwrap();
    pollster::block_on(pulls.put_pull(&record, None)).unwrap();
    let mark = Reconciled {
        repository: Some(key),
        at: Some(now),
        ..Reconciled::default()
    };
    pollster::block_on(pulls.put_reconciled(&herald_core::SourceName::new("cli"), &mark, None))
        .unwrap();
}

/// The config reading `secret_reviewed`'s repository kept, its view showing
/// its load over three teams: one shown, two folded into "other".
fn loaded() -> String {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"acme/secret-plans\"\n",
    );
    format!(
        "{config}[sources.cli.areas]\nmerger-launch = [\"plans/**\"]\n[teams.\"acme/merger-team\"]\nmembers = [\"ana-secret\", \"bo-secret\", \"cy-secret\"]\n[teams.duo-secret]\nmembers = [\"ana-secret\", \"dee-secret\"]\n[teams.pair-secret]\nmembers = [\"cy-secret\", \"eve-secret\"]\n[views.kept]\nsources = [\"cli\"]\nmode = \"kept\"\nreconcile = \"1d\"\nload = {{}}\n"
    )
}

#[test]
fn a_private_repository_s_load_names_no_team_area_or_login_and_counts_nothing_unless_said_private()
{
    let world = World::default();
    secret_reviewed(&world);
    let printed = |setup: &Setup, args: &[&str]| {
        let run = herald(setup, &world, args);
        assert_eq!(run.code, DONE, "{}", run.err);
        format!("{}{}", run.out, run.err)
    };
    let said = loaded();
    let setup = setup(&said);
    let json = printed(&setup, &["queue", "kept", "--format", "json", "--private"]);
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        read["overviews"]["load"],
        json!({
            "teams": [{ "name": "acme/merger-team", "requested": 2, "reviewed": 2 }],
            "areas": [{ "name": "merger-launch", "kind": "paths", "requested": 2, "reviewed": 2 }],
            "other": { "requested": 1, "reviewed": 1 },
        })
    );
    let table = printed(&setup, &["queue", "kept", "--private"]);
    let lines: Vec<&str> = table.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.starts_with("load ("))
        .unwrap_or_else(|| panic!("{table}"));
    assert_eq!(
        lines[at..],
        [
            "load (review requests open, reviews given over the window)",
            "  team acme/merger-team: 2 requested, 2 reviewed",
            "  area merger-launch (paths): 2 requested, 2 reviewed",
            "  other: 1 requested, 1 reviewed",
        ]
    );
    let configured = super::world::setup(&format!("{said}[cli]\nprivate = true\n"));
    for format in ["table", "json"] {
        let shown = printed(&configured, &["queue", "kept", "--format", format]);
        assert!(shown.contains("acme/merger-team"), "{shown}");
    }
    let table = printed(&setup, &["queue", "kept"]);
    assert_eq!(
        table.lines().collect::<Vec<_>>(),
        [
            "kept: 0 open pull requests, 0 stale",
            "1 pull requests of private or internal repositories withheld: --private, or [cli] private, prints them",
            "load (review requests open, reviews given over the window)",
        ]
    );
    let json = printed(&setup, &["queue", "kept", "--format", "json"]);
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        read["overviews"]["load"],
        json!({ "teams": [], "areas": [], "other": null })
    );
    let withheld = format!("{table}{json}");
    for named in SECRET_LOAD {
        assert!(!withheld.contains(named), "{named}: {withheld}");
    }
}

#[test]
fn a_view_read_live_counts_review_requests_alone_and_says_why() {
    let setup = setup(&format!(
        "{CONFIG}[teams.t]\nmembers = [\"ann\", \"ben\", \"cat\"]\n[views.live]\nmost_pulls = 0\nmode = \"live\"\nload = {{}}\n"
    ));
    let world = World::default();
    let pulls = json!([{
        "number": 7, "title": "Add a queue",
        "html_url": "https://github.com/cli/cli/pull/7",
        "created_at": "2026-10-01T12:00:00Z", "updated_at": "2026-10-01T12:00:00Z",
        "user": { "login": "wessel", "type": "User" },
        "requested_reviewers": [{ "login": "Ann", "type": "User" }]
    }]);
    world.state.lock().unwrap().forge.insert(
        "/repos/cli/cli/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1"
            .to_owned(),
        (200, pulls.to_string()),
    );
    let table = herald(&setup, &world, &["queue", "live", "--private"]);
    assert_eq!((table.code, table.err.as_str()), (DONE, ""));
    let lines: Vec<&str> = table.out.lines().collect();
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "load (review requests open; reviews given are counted where every source is read kept)",
            "  team t: 1 requested",
        ]
    );
    let json = herald(
        &setup,
        &world,
        &["queue", "live", "--format", "json", "--private"],
    );
    let read: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(
        read["overviews"]["load"],
        json!({ "teams": [{ "name": "t", "requested": 1, "reviewed": null }], "areas": [], "other": null })
    );
}

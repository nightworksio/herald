//! A view reading an owner source: one query over the owner's repositories,
//! each pull request kept only where its membership record holds its
//! repository, live or kept (ADR-0062).

use herald_config::{Config, Uses, load};
use herald_core::{
    Board, Listed, Listing, Membership, Owner, PullKey, RepositoryKey, SourceName, Visibility,
};
use herald_flows::{Budget, Connected, Ports, Requests, Stop, prepare};
use herald_ports::{Cursors, Pulls, Response};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::harness::{Failing, NOON, Script, healthy};

const SEARCH: &str =
    "/search/issues?q=is%3Apr%20is%3Aopen%20org%3Ao&sort=updated&order=desc&per_page=100&page=1";
const GRAPHQL: &str = "https://api.github.com/graphql";

fn config(view: &str) -> Config {
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[sources.org]\nkind = \"github\"\nowner = \"o\"\ntoken = {{ secret = \"HERALD_GITHUB_TOKEN\" }}\nwebhook = {{ secret = \"HERALD_WEBHOOK\" }}\nvisibility = [\"public\", \"private\"]\n[views.queue]\n{view}\n"
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn member(id: &str, name: &str, visibility: Visibility) -> Listed {
    Listed {
        id: id.to_owned(),
        name: name.to_owned(),
        path: format!("o/{name}"),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

/// `store` holding the owner's listing, `app` (public) and `vault`
/// (private) as members, and `gone`, which left.
fn listed(store: &Failing) {
    let source = SourceName::new("org");
    let listing = Listing {
        owner: Owner::Organization,
        pages: Vec::new(),
        first: NOON,
        listed: NOON,
        fingerprint: String::new(),
        file: None,
        events: None,
        stale: false,
    };
    pollster::block_on(store.put_listing(&source, &listing, None)).unwrap();
    for (repository, left) in [
        (member("1", "app", Visibility::Public), None),
        (member("2", "vault", Visibility::Private), None),
        (member("3", "gone", Visibility::Public), Some(NOON)),
    ] {
        let membership = Membership {
            repository,
            discovered: NOON,
            left,
            cursor: None,
            collides: None,
            hooked: false,
        };
        pollster::block_on(store.put_member(&source, &membership, None)).unwrap();
    }
}

fn found(number: u64, repository: &str) -> Value {
    json!({ "number": number, "title": "t",
        "html_url": format!("https://github.com/o/{repository}/pull/{number}"),
        "created_at": "2026-10-30T10:00:00Z", "updated_at": "2026-10-30T11:00:00Z",
        "repository_url": format!("https://api.github.com/repos/o/{repository}") })
}

fn node(number: u64, repository: &str) -> Value {
    json!({ "number": number, "title": format!("read {number}"),
        "url": format!("https://github.com/o/{repository}/pull/{number}"), "isDraft": false,
        "createdAt": "2026-10-30T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z",
        "headRefOid": format!("h{number}") })
}

fn answer(line: &str, status: u16, etag: Option<&str>, body: &Value) -> (String, Response) {
    (
        line.to_owned(),
        Response {
            status,
            headers: etag
                .map(|etag| ("ETag".to_owned(), etag.to_owned()))
                .into_iter()
                .collect(),
            body: if status == 304 {
                Vec::new()
            } else {
                body.to_string().into_bytes()
            },
        },
    )
}

/// The search finding `app#1`, `vault#2` and `gone#3`, then GraphQL
/// reading the first two.
fn searched() -> Script {
    Script::answered(vec![
        answer(
            SEARCH,
            200,
            Some("\"a\""),
            &json!({ "total_count": 3,
                "items": [found(1, "app"), found(2, "vault"), found(3, "gone")] }),
        ),
        answer(
            GRAPHQL,
            200,
            None,
            &json!({ "data": {
                "r0": { "visibility": "PUBLIC", "p0": node(1, "app") },
                "r1": { "visibility": "PUBLIC", "p0": node(2, "vault") }
            } }),
        ),
    ])
}

fn queued(config: &Config, store: &Failing, script: &Script) -> Result<Board, Stop> {
    queued_within(config, store, script, None)
}

/// The board of the view `queue`, its outbound requests bounded to `most`
/// where it says.
fn queued_within(
    config: &Config,
    store: &Failing,
    script: &Script,
    most: Option<usize>,
) -> Result<Board, Stop> {
    let secrets = FakeSecrets::default()
        .with("HERALD_GITHUB_TOKEN", "ghp_x")
        .with("HERALD_WEBHOOK", "hook-secret");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget {
            requests: most.map(|most| Requests {
                made: script.made(),
                from: script.made().load(std::sync::atomic::Ordering::Relaxed),
                most,
                spare: 0,
            }),
            ..Budget::UNBOUNDED
        },
    };
    pollster::block_on(herald_flows::queue(config, &connected, &ports, "queue"))
}

fn rows(board: &Board) -> Vec<(u64, String, Visibility)> {
    let mut rows: Vec<(u64, String, Visibility)> = board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| {
            (
                row.pull.number,
                row.pull.project.clone(),
                row.pull.visibility,
            )
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn a_live_view_of_an_owner_reads_its_members_pull_requests_by_one_search() {
    let config = config("mode = \"live\"");
    let store = healthy();
    listed(&store);
    let script = searched();
    let board = queued(&config, &store, &script).unwrap();
    assert_eq!(
        rows(&board),
        [
            (1, "app".to_owned(), Visibility::Public),
            (2, "vault".to_owned(), Visibility::Private),
        ]
    );
    assert_eq!(script.seen().len(), 2);
}

#[test]
fn every_request_an_owner_s_read_makes_is_charged_and_none_is_made_past_the_budget() {
    for mode in ["mode = \"live\"", "mode = \"kept\""] {
        let config = config(mode);
        let store = healthy();
        listed(&store);
        let script = searched();
        let Err(Stop::Limit(why)) = queued_within(&config, &store, &script, Some(1)) else {
            panic!("{mode}: a read of two requests stops at a budget of one");
        };
        assert!(
            why.contains("1 of the 1 outbound requests"),
            "{mode}: {why}"
        );
        assert_eq!(script.seen().len(), 1, "{mode}");
    }
}

#[test]
fn a_view_of_an_owner_never_listed_reads_nothing_and_says_why() {
    let config = config("mode = \"live\"");
    let store = healthy();
    let script = Script::default();
    let Err(Stop::Source(why)) = queued(&config, &store, &script) else {
        panic!("an owner never listed is not read");
    };
    assert!(why.contains("never listed"), "{why}");
    assert_eq!(script.seen().len(), 0);
}

fn key(id: &str, number: u64) -> PullKey {
    PullKey {
        repository: RepositoryKey {
            host: "github.com".to_owned(),
            id: id.to_owned(),
        },
        number,
    }
}

#[test]
fn a_kept_view_of_an_owner_keeps_each_member_s_records_and_forgets_a_member_that_left() {
    let config = config("mode = \"kept\"");
    let store = healthy();
    listed(&store);
    let board = queued(&config, &store, &searched()).unwrap();
    assert_eq!(rows(&board).len(), 2);
    let mark = pollster::block_on(store.reconciled(&SourceName::new("org")))
        .unwrap()
        .unwrap()
        .value;
    let held: Vec<&str> = mark
        .repositories
        .iter()
        .map(|key| key.id.as_str())
        .collect();
    assert_eq!(held, ["1", "2"]);
    let vault = pollster::block_on(store.pull(&key("2", 2)))
        .unwrap()
        .unwrap();
    assert_eq!(vault.value.pull.visibility, Visibility::Private);
    let quiet = Script::answered(vec![answer(SEARCH, 304, None, &Value::Null)]);
    assert_eq!(rows(&queued(&config, &store, &quiet).unwrap()).len(), 2);
    assert_eq!(quiet.seen().len(), 1);
    let source = SourceName::new("org");
    let app = pollster::block_on(store.member(&source, "1"))
        .unwrap()
        .unwrap();
    let left = Membership {
        left: Some(NOON),
        ..app.value
    };
    pollster::block_on(store.put_member(&source, &left, Some(app.version))).unwrap();
    let after = Script::answered(vec![answer(
        SEARCH,
        200,
        Some("\"b\""),
        &json!({ "total_count": 1, "items": [found(2, "vault")] }),
    )]);
    let board = queued(&config, &store, &after).unwrap();
    assert_eq!(rows(&board), [(2, "vault".to_owned(), Visibility::Private)]);
    let closed = pollster::block_on(store.pull(&key("1", 1)))
        .unwrap()
        .unwrap();
    assert!(!closed.value.open());
    let mark = pollster::block_on(store.reconciled(&source))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(mark.repositories.len(), 1);
}

#[test]
fn a_kept_owner_s_records_are_seen_as_strictly_as_its_membership_record_says_now() {
    let config = config("mode = \"kept\"\nreconcile = \"1h\"");
    let store = healthy();
    listed(&store);
    queued(&config, &store, &searched()).unwrap();
    let source = SourceName::new("org");
    let app = pollster::block_on(store.member(&source, "1"))
        .unwrap()
        .unwrap();
    let hidden = Membership {
        repository: member("1", "app", Visibility::Private),
        ..app.value
    };
    pollster::block_on(store.put_member(&source, &hidden, Some(app.version))).unwrap();
    let board = queued(&config, &store, &Script::default()).unwrap();
    assert_eq!(
        rows(&board),
        [
            (1, "app".to_owned(), Visibility::Private),
            (2, "vault".to_owned(), Visibility::Private),
        ]
    );
}

#[test]
fn a_kept_owner_listing_as_many_as_it_lists_closes_nothing_it_did_not_list() {
    let config = config("mode = \"kept\"\nmost_open = 2");
    let store = healthy();
    listed(&store);
    let full = Script::answered(vec![
        answer(
            SEARCH,
            200,
            Some("\"a\""),
            &json!({ "total_count": 2, "items": [found(1, "app"), found(2, "vault")] }),
        ),
        answer(
            GRAPHQL,
            200,
            None,
            &json!({ "data": {
                "r0": { "visibility": "PUBLIC", "p0": node(1, "app") },
                "r1": { "visibility": "PUBLIC", "p0": node(2, "vault") }
            } }),
        ),
    ]);
    queued(&config, &store, &full).unwrap();
    let moved = Script::answered(vec![
        answer(
            SEARCH,
            200,
            Some("\"b\""),
            &json!({ "total_count": 3, "items": [found(2, "vault"), found(4, "vault")] }),
        ),
        answer(
            GRAPHQL,
            200,
            None,
            &json!({ "data": { "r0": { "visibility": "PUBLIC", "p0": node(4, "vault") } } }),
        ),
    ]);
    queued(&config, &store, &moved).unwrap();
    let app = pollster::block_on(store.pull(&key("1", 1)))
        .unwrap()
        .unwrap();
    assert!(app.value.open());
}

#[test]
fn a_whole_owner_listing_closes_what_it_no_longer_names_and_keeps_what_it_names_unchanged() {
    let config = config("mode = \"kept\"");
    let store = healthy();
    listed(&store);
    queued(&config, &store, &searched()).unwrap();
    let smaller = Script::answered(vec![answer(
        SEARCH,
        200,
        Some("\"b\""),
        &json!({ "total_count": 1, "items": [found(1, "app")] }),
    )]);
    let board = queued(&config, &store, &smaller).unwrap();
    assert_eq!(smaller.seen().len(), 1);
    assert_eq!(rows(&board), [(1, "app".to_owned(), Visibility::Public)]);
    let app = pollster::block_on(store.pull(&key("1", 1)))
        .unwrap()
        .unwrap();
    assert_eq!(app.value.pull.title, "read 1");
    let vault = pollster::block_on(store.pull(&key("2", 2)))
        .unwrap()
        .unwrap();
    assert!(!vault.value.open());
}

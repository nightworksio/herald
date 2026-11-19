//! A view reading kept: its source's listing reconciled into the records
//! the store keeps, each changed pull request read in full, and the view
//! read back from the records (ADR-0061).

use herald_config::{Config, Uses, load};
use herald_core::{Board, Checks, PullKey, Reconciled, RepositoryKey, SourceName, Visibility};
use herald_flows::{Connected, Ports, Stop, prepare};
use herald_ports::{Pulls, Response};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::{Value, json};

use crate::harness::{Failing, Fault, NOON, Script, healthy};

const LISTING: &str = "/repos/o/r/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1";
const GRAPHQL: &str = "https://api.github.com/graphql";

fn config(view: &str, token: bool) -> Config {
    let token = if token {
        "token = { secret = \"HERALD_GITHUB_TOKEN\" }\n"
    } else {
        ""
    };
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[sources.repo]\nkind = \"github\"\nrepository = \"o/r\"\n{token}webhook = {{ secret = \"HERALD_WEBHOOK\" }}\n[views.queue]\n{view}\n"
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn listed(numbers: &[(u64, &str, &str)]) -> Value {
    listed_as(numbers, false)
}

/// The listing of `numbers`, of a repository private where `private` says.
fn listed_as(numbers: &[(u64, &str, &str)], private: bool) -> Value {
    Value::from(
        numbers
            .iter()
            .map(|(number, head, updated)| {
                json!({ "number": number, "title": "t", "html_url": format!("https://github.com/o/r/pull/{number}"),
                    "created_at": "2026-10-30T10:00:00Z", "updated_at": updated,
                    "head": { "sha": head }, "base": { "repo": { "id": 1, "private": private } } })
            })
            .collect::<Vec<Value>>(),
    )
}

fn visibilities(board: &Board) -> Vec<Visibility> {
    board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| row.pull.visibility)
        .collect()
}

fn node(number: u64, head: &str, updated: &str, rollup: &str) -> Value {
    json!({ "number": number, "title": format!("read {number}"),
        "url": format!("https://github.com/o/r/pull/{number}"), "isDraft": false,
        "createdAt": "2026-10-30T10:00:00Z", "updatedAt": updated, "headRefOid": head,
        "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": rollup,
            "contexts": { "nodes": [] } } } }] } })
}

fn read(nodes: &[Value]) -> Value {
    let mut repository = serde_json::Map::from_iter([("visibility".to_owned(), json!("PUBLIC"))]);
    for (place, node) in nodes.iter().enumerate() {
        repository.insert(format!("p{place}"), node.clone());
    }
    json!({ "data": { "repository": repository } })
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

fn queued(config: &Config, store: &Failing, script: &Script) -> Result<Board, Stop> {
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
        budget: herald_flows::Budget::UNBOUNDED,
    };
    pollster::block_on(herald_flows::queue(config, &connected, &ports, "queue"))
}

fn numbers(board: &Board) -> Vec<u64> {
    let mut numbers: Vec<u64> = board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| row.pull.number)
        .collect();
    numbers.sort_unstable();
    numbers
}

fn repository() -> RepositoryKey {
    RepositoryKey {
        host: "github.com".to_owned(),
        id: "1".to_owned(),
    }
}

fn mark(store: &Failing) -> Reconciled {
    pollster::block_on(store.reconciled(&SourceName::new("repo")))
        .unwrap()
        .unwrap()
        .value
}

#[test]
fn a_kept_view_reconciles_its_listing_into_records_and_reads_them_back() {
    let config = config("", true);
    let store = healthy();
    let first = Script::answered(vec![
        answer(
            LISTING,
            200,
            Some("\"a\""),
            &listed(&[
                (1, "h1", "2026-10-30T11:00:00Z"),
                (2, "h2", "2026-10-30T11:00:00Z"),
            ]),
        ),
        answer(
            GRAPHQL,
            200,
            None,
            &read(&[
                node(1, "h1", "2026-10-30T11:00:00Z", "SUCCESS"),
                node(2, "h2", "2026-10-30T11:00:00Z", "PENDING"),
            ]),
        ),
    ]);
    assert_eq!(numbers(&queued(&config, &store, &first).unwrap()), [1, 2]);
    assert_eq!(mark(&store).repository, Some(repository()));
    assert_eq!(mark(&store).validators[0].etag.as_deref(), Some("\"a\""));
    let unchanged = Script::answered(vec![
        answer(LISTING, 304, None, &Value::Null),
        answer(
            GRAPHQL,
            200,
            None,
            &read(&[node(2, "h2", "2026-10-30T11:00:00Z", "SUCCESS")]),
        ),
    ]);
    assert_eq!(
        numbers(&queued(&config, &store, &unchanged).unwrap()),
        [1, 2]
    );
    assert_eq!(unchanged.seen().len(), 2);
    let key = |number| PullKey {
        repository: repository(),
        number,
    };
    let settled = pollster::block_on(store.pull(&key(2))).unwrap().unwrap();
    assert_eq!(settled.value.pull.checks, Checks::Passed);
    let moved = Script::answered(vec![
        answer(
            LISTING,
            200,
            Some("\"b\""),
            &listed(&[
                (1, "h3", "2026-10-30T12:00:00Z"),
                (2, "h2", "2026-10-30T11:00:00Z"),
            ]),
        ),
        answer(
            GRAPHQL,
            200,
            None,
            &read(&[node(1, "h3", "2026-10-30T12:00:00Z", "SUCCESS")]),
        ),
    ]);
    assert_eq!(numbers(&queued(&config, &store, &moved).unwrap()), [1, 2]);
    let gone = Script::answered(vec![answer(
        LISTING,
        200,
        Some("\"c\""),
        &listed(&[(1, "h3", "2026-10-30T12:00:00Z")]),
    )]);
    assert_eq!(numbers(&queued(&config, &store, &gone).unwrap()), [1]);
    assert_eq!(gone.seen().len(), 1);
    let closed = pollster::block_on(store.pull(&key(2))).unwrap().unwrap();
    assert!(!closed.value.open());
    let quiet = Script::answered(vec![answer(LISTING, 304, None, &Value::Null)]);
    assert_eq!(numbers(&queued(&config, &store, &quiet).unwrap()), [1]);
    assert_eq!(quiet.seen().len(), 1);
}

#[test]
fn a_reconcile_not_yet_due_reads_the_records_alone_and_one_never_listed_reads_nothing() {
    let config = config("reconcile = \"1h\"", true);
    let store = healthy();
    pollster::block_on(store.put_reconciled(
        &SourceName::new("repo"),
        &Reconciled {
            repository: None,
            at: Some(NOON),
            validators: Vec::new(),
        },
        None,
    ))
    .unwrap();
    let script = Script::default();
    assert_eq!(
        numbers(&queued(&config, &store, &script).unwrap()),
        Vec::<u64>::new()
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    let empty = Script::answered(vec![answer(LISTING, 200, None, &json!([]))]);
    let fresh = healthy();
    assert_eq!(
        numbers(&queued(&config, &fresh, &empty).unwrap()),
        Vec::<u64>::new()
    );
    assert_eq!(mark(&fresh).repository, None);
}

#[test]
fn a_listing_that_named_no_repository_and_stands_unchanged_reads_nothing_more() {
    let config = config("", true);
    let store = healthy();
    let empty = Script::answered(vec![answer(LISTING, 200, Some("\"e\""), &json!([]))]);
    assert_eq!(
        numbers(&queued(&config, &store, &empty).unwrap()),
        Vec::<u64>::new()
    );
    let quiet = Script::answered(vec![answer(LISTING, 304, None, &Value::Null)]);
    assert_eq!(
        numbers(&queued(&config, &store, &quiet).unwrap()),
        Vec::<u64>::new()
    );
    assert_eq!(quiet.seen().len(), 1);
    assert_eq!(mark(&store).repository, None);
    assert_eq!(mark(&store).validators[0].etag.as_deref(), Some("\"e\""));
}

#[test]
fn past_the_most_read_in_full_a_listed_pull_request_is_kept_unread_until_a_later_round() {
    let detailed = config("most_pulls = 1", false);
    let store = healthy();
    let script = Script::answered(vec![
        answer(
            LISTING,
            200,
            None,
            &listed(&[
                (1, "h1", "2026-10-30T11:00:00Z"),
                (2, "h2", "2026-10-30T11:00:00Z"),
            ]),
        ),
        answer(
            "/repos/o/r/pulls/1/reviews?per_page=100",
            200,
            None,
            &json!([]),
        ),
        answer(
            "/repos/o/r/commits/h1/check-runs?per_page=100",
            200,
            None,
            &json!({ "check_runs": [] }),
        ),
    ]);
    let board = queued(&detailed, &store, &script).unwrap();
    assert_eq!(numbers(&board), [1, 2]);
    let unread = board
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .find(|row| row.pull.number == 2)
        .unwrap();
    assert!(unread.pull.unread);
    let graphed = Script::answered(vec![
        answer(
            LISTING,
            200,
            None,
            &listed(&[
                (1, "h1", "2026-10-30T11:00:00Z"),
                (2, "h2", "2026-10-30T11:00:00Z"),
            ]),
        ),
        answer(
            GRAPHQL,
            200,
            None,
            &read(&[
                node(1, "h1", "2026-10-30T11:00:00Z", "SUCCESS"),
                node(2, "h2", "2026-10-30T11:00:00Z", "SUCCESS"),
            ]),
        ),
    ]);
    let board = queued(&config("most_pulls = 1", true), &healthy(), &graphed).unwrap();
    assert!(
        board
            .groups
            .iter()
            .flat_map(|group| &group.rows)
            .all(|row| !row.pull.unread)
    );
}

#[test]
fn a_view_reading_kept_stops_where_the_store_keeps_none_or_fails() {
    let kept = config("mode = \"kept\"", true);
    let unkept = Failing {
        fault: Fault::KeepsNoPulls,
        ..healthy()
    };
    assert!(matches!(
        queued(&kept, &unkept, &Script::default()),
        Err(Stop::Store(reason)) if reason.contains("keeps none")
    ));
    let auto = config("", true);
    let live = Script::answered(vec![answer(
        GRAPHQL,
        200,
        None,
        &json!({ "data": { "repository": { "visibility": "PUBLIC", "pullRequests": { "nodes": [] } } } }),
    )]);
    assert_eq!(
        numbers(&queued(&auto, &unkept, &live).unwrap()),
        Vec::<u64>::new()
    );
    let listing = || {
        Script::answered(vec![
            answer(
                LISTING,
                200,
                None,
                &listed(&[(1, "h1", "2026-10-30T11:00:00Z")]),
            ),
            answer(
                GRAPHQL,
                200,
                None,
                &read(&[node(1, "h1", "2026-10-30T11:00:00Z", "SUCCESS")]),
            ),
        ])
    };
    for fault in [
        Fault::ReadsReconciles,
        Fault::ListsPulls,
        Fault::WritesPulls,
        Fault::WritesReconciles,
    ] {
        let store = Failing { fault, ..healthy() };
        assert!(
            matches!(queued(&kept, &store, &listing()), Err(Stop::Store(_))),
            "{fault:?}"
        );
    }
    let refused = Script::answered(vec![answer(LISTING, 500, None, &json!({}))]);
    assert!(matches!(
        queued(&kept, &healthy(), &refused),
        Err(Stop::Source(_))
    ));
    let unread = Script::answered(vec![
        answer(
            LISTING,
            200,
            None,
            &listed(&[(1, "h1", "2026-10-30T11:00:00Z")]),
        ),
        answer(GRAPHQL, 500, None, &json!({})),
    ]);
    assert!(matches!(
        queued(&kept, &healthy(), &unread),
        Err(Stop::Source(_))
    ));
    let raced = Failing {
        fault: Fault::RacesPulls,
        ..healthy()
    };
    assert_eq!(
        numbers(&queued(&kept, &raced, &listing()).unwrap()),
        Vec::<u64>::new()
    );
}

#[test]
fn a_repository_gone_private_between_two_listings_shows_nothing_of_it_as_public() {
    let config = config("", true);
    let store = healthy();
    let pulls = [(1, "h1", "2026-10-30T11:00:00Z")];
    let first = Script::answered(vec![
        answer(LISTING, 200, Some("\"a\""), &listed(&pulls)),
        answer(
            GRAPHQL,
            200,
            None,
            &read(&[node(1, "h1", "2026-10-30T11:00:00Z", "SUCCESS")]),
        ),
    ]);
    assert_eq!(
        visibilities(&queued(&config, &store, &first).unwrap()),
        [Visibility::Public]
    );
    let hidden = Script::answered(vec![answer(
        LISTING,
        200,
        Some("\"b\""),
        &listed_as(&pulls, true),
    )]);
    assert_eq!(
        visibilities(&queued(&config, &store, &hidden).unwrap()),
        [Visibility::Private]
    );
    assert_eq!(hidden.seen().len(), 1);
    let shown = Script::answered(vec![answer(LISTING, 200, Some("\"c\""), &listed(&pulls))]);
    assert_eq!(
        visibilities(&queued(&config, &store, &shown).unwrap()),
        [Visibility::Private]
    );
    let closed = Script::answered(vec![answer(LISTING, 200, None, &json!([]))]);
    assert_eq!(
        numbers(&queued(&config, &store, &closed).unwrap()),
        Vec::<u64>::new()
    );
}

#[test]
fn a_tokenless_rest_listing_s_rows_read_private_live_and_kept_whatever_it_names() {
    let pulls = [(1, "h1", "2026-10-30T11:00:00Z")];
    let script = || {
        Script::answered(vec![
            answer(LISTING, 200, None, &listed(&pulls)),
            answer(
                "/repos/o/r/pulls/1/reviews?per_page=100",
                200,
                None,
                &json!([]),
            ),
            answer(
                "/repos/o/r/commits/h1/check-runs?per_page=100",
                200,
                None,
                &json!({ "check_runs": [] }),
            ),
        ])
    };
    for view in ["mode = \"live\"", "mode = \"kept\""] {
        let board = queued(&config(view, false), &healthy(), &script()).unwrap();
        assert_eq!(visibilities(&board), [Visibility::Private], "{view}");
    }
}

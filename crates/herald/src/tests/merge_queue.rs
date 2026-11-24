use serde_json::{Value, json};

use super::world::*;
use crate::*;

const QUEUE: &str = "[merge_queues.app]\nsource = \"cli\"\nbranch = \"main\"\nruns = \"herald\"\n";

fn pull(number: u64, head: &str) -> Value {
    json!({
        "number": number,
        "html_url": format!("https://github.com/cli/cli/pull/{number}"),
        "draft": false,
        "head": {"sha": head, "ref": format!("topic-{number}"), "repo": {"full_name": "fork/cli"}},
        "user": {"login": "author", "id": 1},
        "labels": [{"name": "herald:merge"}]
    })
}

fn labeled(minute: u32) -> Value {
    json!([{
        "id": minute,
        "event": "labeled",
        "actor": {"login": "writer", "id": 2},
        "created_at": format!("2026-01-01T00:{minute:02}:00Z"),
        "label": {"name": "herald:merge"}
    }])
}

fn answered(world: &World, private: bool) {
    let answers = [
        ("/repos/cli/cli", json!({"private": private})),
        (
            "/repos/cli/cli/branches/main",
            json!({"commit": {"sha": "b1"}, "protected": false}),
        ),
        (
            "/repos/cli/cli/rules/branches/main?per_page=100&page=1",
            json!([]),
        ),
        (
            "/repos/cli/cli/pulls?state=open&base=main&per_page=100&page=1",
            json!([pull(1, "h1"), pull(2, "h2")]),
        ),
        (
            "/repos/cli/cli/issues/1/events?per_page=100&page=1",
            labeled(10),
        ),
        (
            "/repos/cli/cli/issues/2/events?per_page=100&page=1",
            labeled(20),
        ),
        (
            "/repos/cli/cli/collaborators/writer/permission",
            json!({"user": {"permissions": {"pull": true, "push": true}}}),
        ),
        (
            "/repos/fork/cli/activity?ref=refs%2Fheads%2Ftopic-1&direction=desc&per_page=100",
            json!([{"before": "h0", "timestamp": "2026-01-01T00:05:00Z", "activity_type": "push"}]),
        ),
        (
            "/repos/fork/cli/activity?ref=refs%2Fheads%2Ftopic-2&direction=desc&per_page=100",
            json!([{"before": "h0", "timestamp": "2026-01-01T00:05:00Z", "activity_type": "push"}]),
        ),
        (
            "/repos/cli/cli/compare/b1...h1?per_page=1",
            json!({"behind_by": 0}),
        ),
        (
            "/repos/cli/cli/pulls/1",
            json!({"mergeable_state": "clean"}),
        ),
        (
            "/repos/cli/cli/commits/h1/check-runs?per_page=100",
            json!({"total_count": 1, "check_runs": [{"name": "test", "status": "completed", "conclusion": "success"}]}),
        ),
        (
            "/repos/cli/cli/commits/h1/status?per_page=100",
            json!({"total_count": 0, "statuses": []}),
        ),
    ];
    let forge = &mut world.state.lock().unwrap().forge;
    for (path, body) in answers {
        forge.insert(path.to_owned(), (200, body.to_string()));
    }
}

#[test]
fn a_dry_run_reads_each_entry_from_the_forge_and_says_what_comes_next() {
    let setup = setup(&format!(
        "{CONFIG}[merge_queues.dev]\nsource = \"cli\"\nbranch = \"dev\"\n{QUEUE}"
    ));
    let world = World::default();
    answered(&world, false);
    let read = herald(&setup, &world, &["merge-queue", "app"]);
    assert_eq!(
        (
            read.code,
            read.out.lines().collect::<Vec<_>>(),
            read.err.as_str()
        ),
        (
            DONE,
            vec![
                "app: cli/cli into main, run by herald",
                "  takes no step: a merge queue without a method does not start; give method = \"merge\", \"squash\", \"rebase\", \"fast-forward\" or \"repository\"",
                "  takes no step: a merge queue without a write credential does not start; give write = { token = { secret = \"NAME\" } }, or on GitHub write = { app = \"<github_apps name>\" }",
                "  takes no step: runs = \"herald\": herald merge-queue --apply hands pull requests to the forge's queue, and takes no step in herald's own",
                "  #1 https://github.com/cli/cli/pull/1 queued by writer (write) at 2026-01-01T00:10:00Z, head h1: next, it is merged with its head as the expected head",
                "  #2 https://github.com/cli/cli/pull/2 queued by writer (write) at 2026-01-01T00:20:00Z, head h2: waits its turn",
            ],
            ""
        )
    );
    let every = herald(&setup, &world, &["merge-queue"]);
    let app = every.out.find("app: cli/cli into main").unwrap();
    let dev = every.out.find("dev:").unwrap();
    assert!(app < dev, "{}", every.out);
    let unknown = herald(&setup, &world, &["merge-queue", "nope"]);
    assert!(
        unknown
            .err
            .contains("no merge queue is named \"nope\"; the merge queues are \"app\", \"dev\""),
        "{}",
        unknown.err
    );
}

#[test]
fn a_private_repository_s_queue_is_counted_and_a_failed_read_says_why_only_when_private() {
    let setup = setup(&format!("{CONFIG}{QUEUE}"));
    let world = World::default();
    answered(&world, true);
    let withheld = herald(&setup, &world, &["merge-queue", "app"]);
    assert_eq!(withheld.code, DONE);
    assert!(withheld.out.starts_with("app:\n"), "{}", withheld.out);
    assert!(!withheld.out.contains("#1"), "{}", withheld.out);
    let private = herald(&setup, &world, &["merge-queue", "app", "--private"]);
    assert!(
        private.out.contains("#1 https://github.com/cli/cli/pull/1"),
        "{}",
        private.out
    );
    world.state.lock().unwrap().forge.insert(
        "/repos/cli/cli/branches/main".to_owned(),
        (404, "{}".to_owned()),
    );
    let unread = herald(&setup, &world, &["merge-queue"]);
    assert_eq!(
        unread.out.trim(),
        "app:\n  not read; --private, or [cli] private, prints why"
    );
    let said = herald(&setup, &world, &["merge-queue", "--private"]);
    assert_eq!(
        said.out.trim(),
        "app:\n  not read: the forge found no branch"
    );
}

#[test]
fn a_config_without_a_merge_queue_says_how_to_add_one() {
    let setup = setup(CONFIG);
    let world = World::default();
    let read = herald(&setup, &world, &["merge-queue"]);
    assert_eq!(
        (read.code, read.out.trim()),
        (
            DONE,
            "no merge queue is configured; add a [merge_queues.<name>] table"
        )
    );
}

#[test]
fn a_queue_whose_source_s_installation_token_is_not_minted_is_not_read() {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"cli/cli\"\napp = \"herald\"\n",
    ) + QUEUE
        + "[github_apps.herald]\nclient_id = \"Iv23liABCDEF\"\nprivate_key = { secret = \"HERALD_APP_KEY\" }\n";
    let mut minting = setup(&config);
    minting
        .vars
        .insert("HERALD_APP_KEY".to_owned(), super::activitypub::pkcs8());
    let world = World::default();
    answered(&world, false);
    let withheld = herald(&minting, &world, &["merge-queue"]);
    assert_eq!(
        (withheld.code, withheld.out.trim()),
        (
            DONE,
            "app:\n  not read; --private, or [cli] private, prints why"
        )
    );
    let said = herald(&minting, &world, &["merge-queue", "--private"]);
    assert!(
        said.out
            .starts_with("app:\n  not read: its source's installation token could not be minted: "),
        "{}",
        said.out
    );
}

#[test]
fn an_apply_in_a_queue_that_does_not_start_takes_no_step_and_says_why() {
    let setup = setup(&format!("{CONFIG}{QUEUE}"));
    let world = World::default();
    answered(&world, false);
    let applied = herald(&setup, &world, &["merge-queue", "app", "--apply"]);
    assert_eq!(applied.code, DONE, "{}", applied.err);
    assert!(
        applied
            .out
            .contains("  takes no step: a merge queue without a write credential does not start"),
        "{}",
        applied.out
    );
    assert!(
        applied.out.contains("#1 https://github.com/cli/cli/pull/1"),
        "{}",
        applied.out
    );
}

#[test]
fn an_apply_hands_each_pull_request_due_to_the_forge_s_merge_queue() {
    let queue = "[merge_queues.app]\nsource = \"cli\"\nbranch = \"main\"\nmethod = \"squash\"\nwrite = { token = { secret = \"HERALD_MERGE\" } }\n";
    let mut setup = setup(&format!("{CONFIG}{queue}"));
    setup
        .vars
        .insert("HERALD_MERGE".to_owned(), "write-value".to_owned());
    let world = World::default();
    answered(&world, false);
    let fresh = |number: u64, head: &str| {
        let mut fresh = pull(number, head);
        fresh["state"] = json!("open");
        fresh["base"] = json!({ "ref": "main" });
        fresh["node_id"] = json!(format!("PR_{number}"));
        fresh
    };
    let answers = [
        (
            "/repos/cli/cli/rules/branches/main?per_page=100&page=1",
            json!([{ "type": "merge_queue" }]),
        ),
        ("/repos/cli/cli/pulls/1", fresh(1, "h1")),
        ("/repos/cli/cli/pulls/2", fresh(2, "h2")),
        (
            "/graphql",
            json!({ "data": {
                "repository": { "pullRequest": { "isInMergeQueue": false } },
                "enqueuePullRequest": { "mergeQueueEntry": { "id": "MQE" } }
            } }),
        ),
    ];
    {
        let forge = &mut world.state.lock().unwrap().forge;
        for (path, body) in answers {
            forge.insert(path.to_owned(), (200, body.to_string()));
        }
    }
    let applied = herald(&setup, &world, &["merge-queue", "app", "--apply"]);
    assert_eq!(applied.code, DONE, "{}", applied.err);
    assert_eq!(
        applied
            .out
            .matches("handed to the forge's queue, which took it")
            .count(),
        2,
        "{}",
        applied.out
    );
}

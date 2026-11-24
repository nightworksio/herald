//! `herald merge-queue --apply` hands each pull request due to the forge's
//! queue once, with the queue's own write credential alone, after reading
//! it again right before the call and recording the step first.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use herald_config::Uses;
use herald_core::Printed;
use herald_core::merging::{GestureKey, GestureRecord, HandOff, HandedOff};
use herald_flows::{
    Budget, Connected, Found, GONE, Handing, Next, Ports, RACED, SERVED, Took, UNNAMED,
    UNSEPARATED, merge_apply, merge_queues, prepare,
};
use herald_ports::{Actions as _, Base, Http, HttpError, Request, Response};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::{Value, json};

use crate::harness::{Failing, Fault, NOON, config, healthy, secrets};

const QUEUE: &str = r#"[sources.app]
kind = "github"
repository = "o/r"
token = { secret = "HERALD_READ" }
[merge_queues.app]
source = "app"
branch = "main"
method = "squash"
write = { token = { secret = "HERALD_MERGE" } }
"#;

const GRAPHQL: &str = "POST https://api.github.com/graphql";
const PULL: &str = "GET https://api.github.com/repos/o/r/pulls/7";
const EVENTS: &str = "GET https://api.github.com/repos/o/r/issues/7/events?per_page=100&page=1";
const PERMISSION: &str = "GET https://api.github.com/repos/o/r/collaborators/w/permission";

/// A forge answering each request by its method and URL, with the next of
/// its answers to it, the last one standing; and each request it was sent.
/// A request a forge was sent: its line, its headers' values and its body.
type Sent = (String, Vec<String>, Value);

#[derive(Debug, Clone, Default)]
struct Forge {
    answers: Arc<Mutex<BTreeMap<String, VecDeque<Response>>>>,
    sent: Arc<Mutex<Vec<Sent>>>,
}

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

impl Forge {
    fn answering(self, line: &str, answers: &[Response]) -> Self {
        self.answers
            .lock()
            .unwrap()
            .insert(line.to_owned(), answers.iter().cloned().collect());
        self
    }

    /// Each request sent, as its line.
    fn lines(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(line, ..)| line.clone())
            .collect()
    }

    /// Each request whose headers carry `value`, as its line.
    fn carrying(&self, value: &str) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, headers, _)| headers.iter().any(|header| header.contains(value)))
            .map(|(line, ..)| line.clone())
            .collect()
    }

    /// The body of each request sent to `line`.
    fn bodies(&self, line: &str) -> Vec<Value> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(sent, ..)| sent == line)
            .map(|(.., body)| body.clone())
            .collect()
    }
}

impl Http for Forge {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let line = format!("{} {}", request.method.as_str(), request.url.expose());
        let headers = request
            .headers
            .iter()
            .map(|(_, value)| value.expose())
            .collect();
        let body = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        self.sent
            .lock()
            .unwrap()
            .push((line.clone(), headers, body));
        let mut answers = self.answers.lock().unwrap();
        let Some(queued) = answers.get_mut(&line) else {
            return Ok(answer(404, &json!({})));
        };
        if queued.len() > 1 {
            return Ok(queued.pop_front().unwrap());
        }
        queued.front().cloned().ok_or(HttpError::TimedOut)
    }
}

fn pull(head: &str, labels: &[&str]) -> Value {
    json!({
        "number": 7, "html_url": "https://github.com/o/r/pull/7", "node_id": "PR_7",
        "state": "open", "draft": false, "base": { "ref": "main" },
        "head": { "sha": head, "ref": "topic", "repo": { "full_name": "fork/r" } },
        "user": { "login": "author", "id": 1 },
        "labels": labels.iter().map(|name| json!({ "name": name })).collect::<Vec<_>>()
    })
}

fn labeled() -> Value {
    label("labeled", 501, "11:00")
}

fn label(event: &str, id: u64, at: &str) -> Value {
    json!({ "id": id, "event": event, "actor": { "login": "w", "id": 2 },
        "created_at": format!("2026-11-01T{at}:00Z"), "label": { "name": "herald:merge" } })
}

fn writer(push: bool) -> Response {
    answer(
        200,
        &json!({ "user": { "permissions": { "pull": true, "push": push } } }),
    )
}

const ENQUEUED: &str = r#"{"data":{"enqueuePullRequest":{"mergeQueueEntry":{"id":"MQE_1"}}}}"#;

fn enqueued() -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: ENQUEUED.as_bytes().to_vec(),
    }
}

/// The forge with pull request 7, head `h7`, queued by `w`'s label after
/// a push from `h6`, on a branch with a merge queue.
fn scripted() -> Forge {
    Forge::default()
        .answering("GET https://api.github.com/repos/o/r", &[answer(200, &json!({ "private": false }))])
        .answering(
            "GET https://api.github.com/repos/o/r/branches/main",
            &[answer(200, &json!({ "commit": { "sha": "b1" }, "protected": false }))],
        )
        .answering(
            "GET https://api.github.com/repos/o/r/rules/branches/main?per_page=100&page=1",
            &[answer(200, &json!([{ "type": "merge_queue" }]))],
        )
        .answering(
            "GET https://api.github.com/repos/o/r/pulls?state=open&base=main&per_page=100&page=1",
            &[answer(200, &json!([pull("h7", &["herald:merge"])]))],
        )
        .answering(PULL, &[answer(200, &pull("h7", &["herald:merge"]))])
        .answering(EVENTS, &[answer(200, &json!([labeled()]))])
        .answering(PERMISSION, &[writer(true)])
        .answering(
            "GET https://api.github.com/repos/fork/r/activity?ref=refs%2Fheads%2Ftopic&direction=desc&per_page=100",
            &[answer(200, &json!([{ "before": "h6", "timestamp": "2026-11-01T10:00:00Z", "activity_type": "push" }]))],
        )
        .answering(GRAPHQL, &[enqueued()])
}

fn key() -> GestureKey {
    GestureKey {
        api: "https://api.github.com".to_owned(),
        repository: "o/r".to_owned(),
        number: 7,
        gesture: 501,
        by: "w".to_owned(),
        at: "2026-11-01T11:00:00Z".parse().unwrap(),
    }
}

fn the_secrets() -> FakeSecrets {
    secrets()
        .with("HERALD_READ", "read-value")
        .with("HERALD_MERGE", "write-value")
}

/// What the queue's read, applied where `apply`, came to, the store `store`
/// keeping its records, reached through `forge`, with `held` secrets.
fn run(
    extra: &str,
    (store, forge, held): (&Failing, &Forge, &FakeSecrets),
    apply: bool,
) -> Vec<Next> {
    run_within(extra, (store, forge, held), (apply, Budget::UNBOUNDED))
}

/// What [`run`] comes to within `budget`.
fn run_within(
    extra: &str,
    (store, forge, held): (&Failing, &Forge, &FakeSecrets),
    (apply, budget): (bool, Budget<'_>),
) -> Vec<Next> {
    run_config(
        &format!("{QUEUE}{extra}"),
        (store, forge, held),
        (apply, budget),
    )
}

/// What the queue `queued` configures comes to, as [`run_within`] says.
fn run_config(
    queued: &str,
    (store, forge, held): (&Failing, &Forge, &FakeSecrets),
    (apply, budget): (bool, Budget<'_>),
) -> Vec<Next> {
    let config = config(queued);
    let prepared = pollster::block_on(prepare(&config, held, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Forge> = prepared.connect(|_| Ok::<_, ()>(forge.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget,
    };
    let flow = (&config, &connected, &ports);
    let asked = (Some("app"), Printed::Asked);
    let bases = Mutex::new(Vec::new());
    let connect = |base: Base<'_>| {
        if let Base::At(at) = base {
            bases.lock().unwrap().push(at.to_owned());
        }
        Ok(forge.clone())
    };
    let handing = Handing {
        secrets: held,
        connect: &connect,
    };
    let read = if apply {
        pollster::block_on(merge_apply(flow, &handing, asked))
    } else {
        pollster::block_on(merge_queues(flow, asked))
    }
    .unwrap();
    let Found::Entries(entries) = &read[0].found else {
        panic!("{:?}", read[0]);
    };
    let bases = bases.into_inner().unwrap();
    assert!(
        bases.iter().all(|base| base == "https://api.github.com"),
        "{bases:?}"
    );
    if !apply {
        assert_eq!(bases, Vec::<String>::new());
    }
    let mut nexts: Vec<Next> = read[0]
        .idle
        .iter()
        .map(|why| Next::Took(Took::Refused(why.clone())))
        .collect();
    nexts.extend(entries.iter().map(|judged| judged.next.clone()));
    nexts
}

fn record(store: &Failing) -> Option<GestureRecord> {
    pollster::block_on(store.inner.gesture(&key()))
        .unwrap()
        .map(|kept| kept.value)
}

#[test]
fn a_pull_request_due_is_handed_over_once_with_the_write_credential_alone() {
    let store = healthy();
    let forge = scripted();
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), false),
        [Next::HandOff]
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    assert_eq!(record(&store), None);
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
    assert_eq!(forge.carrying("write-value"), [GRAPHQL]);
    assert!(
        forge
            .carrying("read-value")
            .iter()
            .all(|line| line.starts_with("GET "))
    );
    assert_eq!(
        forge.bodies(GRAPHQL)[0]["variables"],
        json!({ "id": "PR_7", "head": "h7" })
    );
    let lines = forge.lines();
    let call = lines.iter().position(|line| line == GRAPHQL).unwrap();
    let fresh = lines[..call].iter().rposition(|line| line == PULL).unwrap();
    assert!(lines[fresh..call].contains(&EVENTS.to_owned()), "{lines:?}");
    assert!(
        lines[fresh..call].contains(&PERMISSION.to_owned()),
        "{lines:?}"
    );
    assert_eq!(
        record(&store).unwrap().hand_off,
        Some(HandOff {
            head: "h7".to_owned(),
            base: "b1".to_owned(),
            credential: "HERALD_MERGE".to_owned(),
            at: NOON,
            outcome: HandedOff::Queued,
        })
    );
    let again = scripted();
    for apply in [false, true] {
        assert_eq!(
            run("", (&store, &again, &the_secrets()), apply),
            [Next::Handed {
                outcome: HandedOff::Queued,
                at: NOON
            }]
        );
    }
    assert_eq!(again.carrying("write-value"), Vec::<String>::new());
}

#[test]
fn the_dry_run_and_every_read_path_need_no_write_credential_and_never_send_it() {
    let store = healthy();
    let forge = scripted();
    let read_only = secrets().with("HERALD_READ", "read-value");
    assert_eq!(
        run("", (&store, &forge, &read_only), false),
        [Next::HandOff]
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    let unwritten = run("", (&store, &forge, &read_only), true);
    assert!(
        matches!(&unwritten[..], [Next::Took(Took::Refused(why))] if why.contains("the write credential could not be read")),
        "{unwritten:?}"
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
    let same = secrets()
        .with("HERALD_READ", "one-value")
        .with("HERALD_MERGE", "one-value");
    assert_eq!(
        run("", (&store, &forge, &same), true),
        [Next::Took(Took::Refused(UNSEPARATED.to_owned()))]
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn a_head_moved_a_role_lost_or_a_cancel_since_the_queue_s_read_hands_nothing_over() {
    let moved = scripted().answering(PULL, &[answer(200, &pull("h8", &["herald:merge"]))]);
    let unroled = scripted().answering(PERMISSION, &[writer(true), writer(false)]);
    let cancelled = scripted().answering(
        EVENTS,
        &[
            answer(200, &json!([labeled()])),
            answer(
                200,
                &json!([labeled(), { "event": "unlabeled", "actor": { "login": "x", "id": 3 },
                "created_at": "2026-11-01T11:30:00Z", "label": { "name": "herald:merge" } }]),
            ),
        ],
    );
    for (forge, took) in [
        (moved, Took::HeadMoved),
        (unroled, Took::Refused(GONE.to_owned())),
        (cancelled, Took::Refused(GONE.to_owned())),
    ] {
        let store = healthy();
        assert_eq!(
            run("", (&store, &forge, &the_secrets()), true),
            [Next::Took(took)]
        );
        assert!(
            !forge.lines().contains(&GRAPHQL.to_owned()),
            "{:?}",
            forge.lines()
        );
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        assert_eq!(record(&store), None);
    }
}

#[test]
fn a_forge_refusal_is_recorded_as_head_moved_where_the_head_read_back_moved() {
    let refused = answer(
        200,
        &json!({ "errors": [{ "message": "Expected head oid does not match" }] }),
    );
    let pulls = [
        answer(200, &pull("h7", &["herald:merge"])),
        answer(200, &pull("h8", &["herald:merge"])),
    ];
    let moved = scripted()
        .answering(GRAPHQL, std::slice::from_ref(&refused))
        .answering(PULL, &pulls);
    let store = healthy();
    assert_eq!(
        run("", (&store, &moved, &the_secrets()), true),
        [Next::Took(Took::HeadMoved)]
    );
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::HeadMoved
    );
    let stood = scripted().answering(GRAPHQL, &[refused]);
    let store = healthy();
    assert_eq!(
        run("", (&store, &stood, &the_secrets()), true),
        [Next::Took(Took::Declined(
            "GitHub refused it: Expected head oid does not match".to_owned()
        ))]
    );
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::Refused
    );
    let again = scripted();
    assert_eq!(
        run("", (&store, &again, &the_secrets()), true),
        [Next::Handed {
            outcome: HandedOff::Refused,
            at: NOON
        }]
    );
    assert!(!again.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn a_lost_answer_is_read_back_and_never_asked_again() {
    let queued = json!({ "data": { "repository": { "pullRequest": { "isInMergeQueue": true } } } });
    let unqueued =
        json!({ "data": { "repository": { "pullRequest": { "isInMergeQueue": false } } } });
    for (back, outcome, took) in [
        (queued, HandedOff::Queued, Took::Queued),
        (unqueued, HandedOff::Unsettled, Took::Unsettled),
    ] {
        let lost = scripted().answering(GRAPHQL, &[answer(502, &json!({})), answer(200, &back)]);
        let store = healthy();
        assert_eq!(
            run("", (&store, &lost, &the_secrets()), true),
            [Next::Took(took)]
        );
        assert_eq!(record(&store).unwrap().hand_off.unwrap().outcome, outcome);
        assert_eq!(lost.carrying("write-value"), [GRAPHQL]);
        assert_eq!(lost.carrying("read-value").last().unwrap(), GRAPHQL);
    }
}

fn kept(store: &Failing, outcome: HandedOff) {
    let record = GestureRecord {
        key: key(),
        queue: "app".to_owned(),
        seen: "h7".to_owned(),
        seen_at: NOON,
        hand_off: Some(HandOff {
            head: "h7".to_owned(),
            base: "b1".to_owned(),
            credential: "HERALD_MERGE".to_owned(),
            at: NOON,
            outcome,
        }),
    };
    pollster::block_on(store.inner.put_gesture(&record, None)).unwrap();
}

#[test]
fn a_step_recorded_before_a_crash_is_never_taken_again_and_one_never_sent_is() {
    let store = healthy();
    kept(&store, HandedOff::Calling);
    let forge = scripted();
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), true),
        [Next::Handed {
            outcome: HandedOff::Calling,
            at: NOON
        }]
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    let store = healthy();
    kept(&store, HandedOff::Unsent);
    let forge = scripted();
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
    assert_eq!(forge.carrying("write-value"), [GRAPHQL]);
}

#[test]
fn a_record_that_cannot_be_written_first_hands_nothing_over_and_one_unsettled_says_so() {
    for (fault, took) in [
        (Fault::ActionsConflict, Took::Refused(RACED.to_owned())),
        (Fault::SettlesNoAction, Took::Unrecorded(HandedOff::Queued)),
    ] {
        let store = Failing { fault, ..healthy() };
        let forge = scripted();
        assert_eq!(
            run("", (&store, &forge, &the_secrets()), true),
            [Next::Took(took)]
        );
    }
    let store = Failing {
        fault: Fault::WritesActions,
        ..healthy()
    };
    let forge = scripted();
    let refused = run("", (&store, &forge, &the_secrets()), true);
    assert!(
        matches!(&refused[..], [Next::Took(Took::Refused(why))] if why.contains("could not be written")),
        "{refused:?}"
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn a_queue_the_server_runs_or_without_a_forge_queue_takes_no_step() {
    let store = healthy();
    let forge = scripted();
    let nexts = run(
        "applied_by = \"server\"\n",
        (&store, &forge, &the_secrets()),
        true,
    );
    assert_eq!(
        nexts,
        [Next::Took(Took::Refused(SERVED.to_owned())), Next::Idle]
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
    assert_eq!(record(&store), None);
    let queueless = scripted().answering(
        "GET https://api.github.com/repos/o/r/rules/branches/main?per_page=100&page=1",
        &[answer(200, &json!([]))],
    );
    let store = healthy();
    let nexts = run("", (&store, &queueless, &the_secrets()), true);
    assert!(
        matches!(&nexts[..], [Next::Took(Took::Refused(why)), Next::Idle] if why.contains("no merge queue")),
        "{nexts:?}"
    );
    assert!(!queueless.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn a_seen_head_is_recorded_by_the_first_apply_and_vouches_from_then_on() {
    let store = healthy();
    let forge = scripted();
    assert_eq!(
        run(
            "vouches = \"seen\"\n",
            (&store, &forge, &the_secrets()),
            false
        ),
        [Next::HandOff]
    );
    assert_eq!(record(&store), None);
    assert_eq!(
        run(
            "vouches = \"seen\"\n",
            (&store, &forge, &the_secrets()),
            true
        ),
        [Next::Took(Took::Queued)]
    );
    let kept = record(&store).unwrap();
    assert_eq!((kept.seen.as_str(), kept.seen_at), ("h7", NOON));
    let store = healthy();
    kept_seen(&store, "h6");
    let moved = scripted();
    assert_eq!(
        run(
            "vouches = \"seen\"\n",
            (&store, &moved, &the_secrets()),
            true
        ),
        [Next::Verdict(herald_core::merging::Verdict::Leaves(
            herald_core::merging::Left::HeadMoved
        ))]
    );
    assert!(!moved.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn a_seen_head_that_cannot_be_read_or_recorded_vouches_for_nothing() {
    for fault in [Fault::ReadsActions, Fault::WritesActions] {
        let store = Failing { fault, ..healthy() };
        let forge = scripted();
        let nexts = run(
            "vouches = \"seen\"\n",
            (&store, &forge, &the_secrets()),
            true,
        );
        assert_eq!(
            nexts,
            [Next::Verdict(herald_core::merging::Verdict::Leaves(
                herald_core::merging::Left::Unrecorded
            ))],
            "{fault:?}"
        );
        assert_eq!(record(&store), None);
        assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
    }
}

fn kept_seen(store: &Failing, seen: &str) {
    let record = GestureRecord {
        key: key(),
        queue: "app".to_owned(),
        seen: seen.to_owned(),
        seen_at: NOON,
        hand_off: None,
    };
    pollster::block_on(store.inner.put_gesture(&record, None)).unwrap();
}

#[test]
fn a_gesture_made_again_after_one_was_refused_is_another_record_and_is_handed_over() {
    let store = healthy();
    kept(&store, HandedOff::Refused);
    let again = scripted().answering(
        EVENTS,
        &[answer(
            200,
            &json!([
                labeled(),
                label("unlabeled", 502, "11:10"),
                label("labeled", 503, "11:20")
            ]),
        )],
    );
    assert_eq!(
        run("", (&store, &again, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
    let made_again = GestureKey {
        gesture: 503,
        at: "2026-11-01T11:20:00Z".parse().unwrap(),
        ..key()
    };
    let kept_again = pollster::block_on(store.inner.gesture(&made_again))
        .unwrap()
        .unwrap();
    assert_eq!(
        kept_again.value.hand_off.unwrap().outcome,
        HandedOff::Queued
    );
    let same_second = scripted().answering(
        EVENTS,
        &[answer(200, &json!([label("labeled", 504, "11:00")]))],
    );
    let store = healthy();
    kept(&store, HandedOff::Refused);
    assert_eq!(
        run("", (&store, &same_second, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
}

#[test]
fn a_gesture_by_one_without_the_role_is_no_entry_and_leaves_no_record() {
    let unroled = scripted().answering(PERMISSION, &[writer(false)]);
    let store = healthy();
    for apply in [false, true] {
        let read = run("", (&store, &unroled, &the_secrets()), apply);
        assert_eq!(read, Vec::<Next>::new());
    }
    assert_eq!(record(&store), None);
    assert!(!unroled.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn a_pull_request_that_cannot_be_read_again_or_a_call_the_budget_never_sent_hands_nothing_over() {
    let unread = scripted().answering(PULL, &[answer(500, &json!({}))]);
    let store = healthy();
    let refused = run("", (&store, &unread, &the_secrets()), true);
    assert!(
        matches!(&refused[..], [Next::Took(Took::Refused(why))] if why.starts_with("it could not be read again right before the step")),
        "{refused:?}"
    );
    assert!(!unread.lines().contains(&GRAPHQL.to_owned()));
    assert_eq!(record(&store), None);
    let store = healthy();
    let spent = scripted();
    let budget = Budget {
        until: Some(NOON),
        ..Budget::UNBOUNDED
    };
    assert_eq!(
        run_within("", (&store, &spent, &the_secrets()), (true, budget)),
        [Next::Took(Took::Refused(
            HandedOff::Unsent.said().to_owned()
        ))]
    );
    assert!(!spent.lines().contains(&GRAPHQL.to_owned()));
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::Unsent
    );
    let again = scripted();
    assert_eq!(
        run("", (&store, &again, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
}

#[test]
fn a_queue_that_takes_no_step_records_no_first_seen_head() {
    let store = healthy();
    let forge = scripted();
    let nexts = run(
        "applied_by = \"server\"\nvouches = \"seen\"\n",
        (&store, &forge, &the_secrets()),
        true,
    );
    assert_eq!(
        nexts,
        [Next::Took(Took::Refused(SERVED.to_owned())), Next::Idle]
    );
    assert_eq!(record(&store), None);
}

#[test]
fn a_gesture_the_forge_names_no_id_for_is_never_handed_over() {
    let unnamed = json!({ "event": "labeled", "actor": { "login": "w", "id": 2 },
        "created_at": "2026-11-01T11:00:00Z", "label": { "name": "herald:merge" } });
    let forge = scripted().answering(EVENTS, &[answer(200, &json!([unnamed]))]);
    let store = healthy();
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), true),
        [Next::Took(Took::Refused(UNNAMED.to_owned()))]
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn a_call_that_cannot_be_made_or_an_app_token_never_minted_is_recorded_unsent() {
    let mut nameless = pull("h7", &["herald:merge"]);
    nameless["node_id"] = Value::Null;
    let forge = scripted().answering(PULL, &[answer(200, &nameless)]);
    let store = healthy();
    let refused = run("", (&store, &forge, &the_secrets()), true);
    assert!(
        matches!(&refused[..], [Next::Took(Took::Refused(why))] if why.contains("no node id")),
        "{refused:?}"
    );
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::Unsent
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
    let app = "[github_apps.merger]\nclient_id = \"Iv23liMerger\"\nprivate_key = { secret = \"HERALD_MERGER_KEY\" }\n";
    let as_app = format!("{QUEUE}{app}").replace(
        "write = { token = { secret = \"HERALD_MERGE\" } }",
        "write = { app = \"merger\" }",
    );
    let store = healthy();
    let forge = scripted();
    let held = the_secrets().with("HERALD_MERGER_KEY", "the App's key");
    let unminted = run_config(&as_app, (&store, &forge, &held), (true, Budget::UNBOUNDED));
    assert!(
        matches!(&unminted[..], [Next::Took(Took::Refused(why))] if why.starts_with("the write App's token could not be minted")),
        "{unminted:?}"
    );
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::Unsent
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn an_entry_not_due_passes_by_the_hand_off_untouched() {
    let mut draft = pull("h8", &["herald:merge"]);
    draft["number"] = json!(8);
    draft["draft"] = json!(true);
    let forge = scripted()
        .answering(
            "GET https://api.github.com/repos/o/r/pulls?state=open&base=main&per_page=100&page=1",
            &[answer(200, &json!([pull("h7", &["herald:merge"]), draft]))],
        )
        .answering(
            "GET https://api.github.com/repos/o/r/issues/8/events?per_page=100&page=1",
            &[answer(200, &json!([label("labeled", 801, "11:30")]))],
        );
    let store = healthy();
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), true),
        [Next::Took(Took::Queued), Next::Draft]
    );
}

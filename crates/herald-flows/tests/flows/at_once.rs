use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use herald_config::{Config, Uses};
use herald_core::{RouteId, SourceName};
use herald_flows::{Announced, Outcome, Ports, Sweep, announce, poll, prepare};
use herald_ports::{Announcements, Http, HttpError, Request, Response};
use herald_testing::{FakeClock, FakeSigner};

use crate::harness::{Failing, Fault, NOON, event, healthy, loaded, secrets};

/// One poll's wait, after which a future goes on.
struct Turn(bool);

impl Future for Turn {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        context.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Answers each request by the end of its URL, after a turn, and keeps
/// each request's method and path in the order they were sent.
#[derive(Clone, Default)]
struct ByPath {
    answers: Vec<(&'static str, &'static str)>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl ByPath {
    fn answering(answers: &[(&'static str, &'static str)]) -> Self {
        Self {
            answers: answers.to_vec(),
            seen: Arc::default(),
        }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl Http for ByPath {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        Turn(false).await;
        let url = request.url.redacted();
        let path = url
            .split_once(".com")
            .map_or(url.as_str(), |(_, path)| path);
        let path = path.strip_prefix("/api/v10").unwrap_or(path);
        self.seen
            .lock()
            .unwrap()
            .push(format!("{} {path}", request.method.as_str()));
        let (_, body) = self
            .answers
            .iter()
            .find(|(end, _)| path.ends_with(end))
            .ok_or_else(|| HttpError::Unreachable {
                reason: format!("nothing answers {path}"),
            })?;
        Ok(Response {
            status: 200,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        })
    }
}

const FIRST: &str = "/channels/1566200000000000100/messages";
const SECOND: &str = "/channels/1566200000000000200/messages";
const POSTED: &str = r#"{"id":"1"}"#;

fn routed(run: &str) -> Config {
    loaded(&format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "first"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
crosspost = true
[[routes]]
id = "second"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000200"
template = "release"
crosspost = true
{run}
[templates.release]
title = "{{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{{{ event.body }}}}"
"#
    ))
}

fn routes_answering() -> ByPath {
    ByPath::answering(&[
        (FIRST, POSTED),
        (SECOND, POSTED),
        ("/messages/1/crosspost", POSTED),
    ])
}

fn announced(config: &Config, store: &Failing, http: &ByPath) -> Vec<Announced> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(http.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
    };
    pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        &event("v1.0.0"),
        "r1",
    ))
}

fn sent(announced: &[Announced]) -> Vec<(&str, &Outcome)> {
    announced
        .iter()
        .map(|announced| (announced.route.as_str(), &announced.outcome))
        .collect()
}

#[test]
fn routes_are_announced_to_at_once_and_answer_in_the_file_s_order() {
    let http = routes_answering();
    let store = healthy();
    let announced = announced(&routed(""), &store, &http);
    let landed = Outcome::Sent {
        steps: vec![herald_core::Step::Post, herald_core::Step::Crosspost],
    };
    assert_eq!(sent(&announced), [("first", &landed), ("second", &landed)]);
    assert_eq!(
        http.seen(),
        [
            format!("POST {FIRST}"),
            format!("POST {SECOND}"),
            format!("POST {FIRST}/1/crosspost"),
            format!("POST {SECOND}/1/crosspost"),
        ]
    );
    let latest = pollster::block_on(store.inner.latest("php-mutation-gate"))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(
        latest.routes.iter().collect::<Vec<_>>(),
        [&RouteId::new("first"), &RouteId::new("second")]
    );
}

#[test]
fn one_route_at_a_time_goes_in_the_file_s_order_as_does_a_store_writing_one_record_at_a_time() {
    let one_after_another = [
        format!("POST {FIRST}"),
        format!("POST {FIRST}/1/crosspost"),
        format!("POST {SECOND}"),
        format!("POST {SECOND}/1/crosspost"),
    ];
    let http = routes_answering();
    announced(&routed("[run]\nroutes = 1\n"), &healthy(), &http);
    assert_eq!(http.seen(), one_after_another);
    let http = routes_answering();
    let store = Failing {
        fault: Fault::OneAtATime,
        ..healthy()
    };
    announced(&routed(""), &store, &http);
    assert_eq!(http.seen(), one_after_another);
}

fn polled(run: &str, store: &Failing) -> (Vec<herald_flows::Swept>, ByPath) {
    let config = loaded(&format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
backfill = 1
[sources.mirror]
kind = "github"
repository = "nightworksio/php-mutation-gate"
backfill = 1
[[routes]]
id = "first"
sources = ["gate", "mirror"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
{run}
[templates.release]
title = "{{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{{{ event.body }}}}"
"#
    ));
    let releases = r#"[{"tag_name":"v1.0.0","name":"php-mutation-gate v1.0.0","body":"Added","draft":false,"prerelease":false,"published_at":"2026-10-31T12:00:00Z","html_url":"https://github.com/nightworksio/php-mutation-gate/releases/tag/v1.0.0"}]"#;
    let http = ByPath::answering(&[
        ("/releases?per_page=30", releases),
        (
            "/repos/nightworksio/php-mutation-gate",
            r#"{"id":1,"name":"php-mutation-gate"}"#,
        ),
        (FIRST, POSTED),
    ]);
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(http.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
    };
    (
        pollster::block_on(poll(&config, &connected, &ports, "r1")),
        http,
    )
}

fn outcomes(swept: &[herald_flows::Swept]) -> Vec<(&str, Vec<&Outcome>)> {
    swept
        .iter()
        .map(|swept| {
            let Sweep::Polled { announced, .. } = &swept.outcome else {
                panic!("{swept:?}");
            };
            (
                swept.source.as_str(),
                announced
                    .iter()
                    .flat_map(|fresh| fresh.announced.iter().map(|announced| &announced.outcome))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn sources_finding_one_event_at_once_send_it_once_and_the_other_is_told_it_is_held() {
    let store = healthy();
    let (swept, http) = polled("", &store);
    let posts = http
        .seen()
        .into_iter()
        .filter(|seen| seen.starts_with("POST"))
        .count();
    assert_eq!(posts, 1);
    let outcomes = outcomes(&swept);
    assert_eq!(outcomes[0].0, "gate");
    assert_eq!(outcomes[1].0, "mirror");
    assert!(
        matches!(outcomes[0].1[..], [Outcome::Sent { .. }]),
        "{outcomes:?}"
    );
    assert!(
        matches!(&outcomes[1].1[..], [Outcome::Held { by, .. }] if by == "r1/gate"),
        "{outcomes:?}"
    );
}

#[test]
fn one_source_at_a_time_finds_the_event_announced_by_the_one_before() {
    let store = healthy();
    let (swept, http) = polled("[run]\nsources = 1\n", &store);
    let outcomes = outcomes(&swept);
    assert!(
        matches!(outcomes[0].1[..], [Outcome::Sent { .. }]),
        "{outcomes:?}"
    );
    assert!(
        matches!(outcomes[1].1[..], [Outcome::AlreadyAnnounced]),
        "{outcomes:?}"
    );
    let posts = http
        .seen()
        .into_iter()
        .filter(|seen| seen.starts_with("POST"))
        .count();
    assert_eq!(posts, 1);
}

//! A pull request event's areas, read from the paths kept for its head or
//! from the forge as it is delivered, narrowing the routes that take it
//! and carried with its repository's visibility (ADR-0072).

use std::collections::BTreeMap;

use herald_config::{Config, Uses, load};
use herald_core::{
    Attribute, COMMIT, Changed, Draft, Event, KeptPull, Kind, NUMBER, Origin, PULL_LINK, PullKey,
    RepositoryKey, SIGNAL, Signal, SourceName, Visibility,
};
use herald_flows::{Budget, Ports, announce, prepare};
use herald_ports::Pulls;
use herald_testing::{FakeClock, FakeSigner};

use crate::harness::{Failing, NOON, POSTED, Script, healthy, secrets};

const FILES: &str = "https://api.github.com/repos/o/r/pulls/4/files?per_page=100";

fn config(paths: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.alpha]
kind = "github"
repository = "o/r"
api = "https://ghe.example/api/v3"
token = {{ secret = "HERALD_FORGE_TOKEN" }}
[sources.alpha.areas]
docs = ["docs/**"]
[sources.gate]
kind = "github"
repository = "o/r"
signals = ["pull-request"]
token = {{ secret = "HERALD_FORGE_TOKEN" }}
[sources.gate.areas]
web = ["web/**"]
docs = ["docs/**"]
secret = ["secret/**"]
[[routes]]
id = "web"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
private = true
{paths}
match = {{ areas = ["web"] }}
[[routes]]
id = "open"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
{paths}
match = {{ not = {{ areas = ["secret"] }} }}
[[routes]]
id = "elsewhere"
sources = ["alpha"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
area_paths = "delivery"
match = {{ areas = ["docs"] }}
[[routes]]
id = "all"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
[templates.t]
title = "{{{{ event.title }}}}"
body = "zone {{% if event.origin.areas %}}{{{{ event.origin.areas | join(' ') }}}}{{% else %}}none{{% endif %}} enoz"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn event(visibility: Visibility) -> Event {
    let text = |value: &str| Attribute::Text(value.to_owned());
    Event::normalise(Draft {
        identity: "github.com/1/pull/4/opened".to_owned(),
        kind: Kind::PULL_REQUEST,
        project: "r".to_owned(),
        title: "Pull request #4 opened: Add a page".to_owned(),
        body: String::new(),
        links: BTreeMap::from([(
            PULL_LINK.to_owned(),
            "https://github.com/o/r/pull/4".to_owned(),
        )]),
        attributes: BTreeMap::from([
            (SIGNAL.to_owned(), text(Signal::PullRequest.name())),
            (NUMBER.to_owned(), Attribute::Number(4.into())),
            (COMMIT.to_owned(), text("c0ffee")),
        ]),
        occurred: NOON,
    })
    .unwrap()
    .from_origin(Origin {
        forge: "github".to_owned(),
        host: "github.com".to_owned(),
        owner: "o".to_owned(),
        repository: "o/r".to_owned(),
        id: "1".to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility,
        areas: None,
        source: None,
    })
}

fn kept(store: &Failing, head: &str, paths: &[&str], whole: bool) {
    let pull = herald_core::Pull {
        project: "r".to_owned(),
        repository: "o/r".to_owned(),
        visibility: Visibility::Public,
        number: 4,
        title: "Add a page".to_owned(),
        url: "https://github.com/o/r/pull/4".to_owned(),
        author: None,
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: NOON,
        updated: NOON,
        review: herald_core::Review::Unasked,
        reviewers: Vec::new(),
        checks: herald_core::Checks::Absent,
        mergeable: herald_core::Mergeable::Unknown,
        queued: false,
        forge: None,
        head: Some(head.to_owned()),
        unread: false,
        base: None,
        files: Some(Changed {
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
            whole,
        }),
        areas: Vec::new(),
        flips: Vec::new(),
    };
    let key = PullKey {
        repository: RepositoryKey::of(&pull.url, "1").unwrap(),
        number: 4,
    };
    let kept = KeptPull::read(None, key, pull, NOON);
    pollster::block_on(store.put_pull(&kept, None)).unwrap();
}

/// The routes of `config` that announce `event` through `script`, each
/// with the areas its post names.
fn announced(config: &Config, store: &Failing, script: &Script, event: &Event) -> Vec<String> {
    let secrets = secrets().with("HERALD_FORGE_TOKEN", "t");
    let prepared =
        pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Serves)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let announced = pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        event,
        "r1",
    ));
    let bodies: Vec<String> = script
        .bodies()
        .into_iter()
        .filter(|body| !body.is_empty())
        .map(|body| String::from_utf8(body).unwrap())
        .collect();
    announced
        .into_iter()
        .zip(bodies)
        .map(|(announced, body)| {
            let said = body
                .split_once("zone ")
                .and_then(|(_, rest)| rest.split_once(" enoz"))
                .map_or(body.as_str(), |(said, _)| said)
                .to_owned();
            format!("{} {said}", announced.route.as_str())
        })
        .collect()
}

fn posting() -> Script {
    Script::answering(&[POSTED, POSTED, POSTED])
}

#[test]
fn an_event_takes_the_areas_of_the_paths_kept_for_its_head_and_none_of_another_head() {
    let config = config("");
    let store = healthy();
    kept(&store, "c0ffee", &["web/index.html"], true);
    let script = posting();
    assert_eq!(
        announced(&config, &store, &script, &event(Visibility::Public)),
        ["web web", "open web", "all web"]
    );
    assert!(!script.seen().iter().any(|line| line.contains("/files")));
    let store = healthy();
    kept(&store, "older", &["web/index.html"], true);
    assert_eq!(
        announced(&config, &store, &posting(), &event(Visibility::Public)),
        ["all none"]
    );
}

#[test]
fn paths_not_all_kept_or_not_kept_leave_an_event_in_no_area() {
    let config = config("");
    let store = healthy();
    kept(&store, "c0ffee", &["web/index.html"], false);
    assert_eq!(
        announced(&config, &store, &posting(), &event(Visibility::Public)),
        ["all none"]
    );
    assert_eq!(
        announced(&config, &healthy(), &posting(), &event(Visibility::Public)),
        ["all none"]
    );
    let secret = healthy();
    kept(&secret, "c0ffee", &["secret/key.pem", "web/a"], true);
    assert_eq!(
        announced(&config, &secret, &posting(), &event(Visibility::Public)),
        ["web secret web", "all secret web"]
    );
}

#[test]
fn a_route_reading_paths_at_delivery_reads_them_once_and_a_failed_read_leaves_no_area() {
    let config = config("area_paths = \"delivery\"");
    let files = r#"[{"filename":"web/a.ts"},{"filename":"docs/b.md"}]"#;
    let script = Script::keyed(BTreeMap::from([(
        FILES.to_owned(),
        herald_ports::Response {
            status: 200,
            headers: Vec::new(),
            body: files.as_bytes().to_vec(),
        },
    )]))
    .then(&[POSTED, POSTED, POSTED]);
    let read = announced(&config, &healthy(), &script, &event(Visibility::Public));
    assert_eq!(read, ["web docs web", "open docs web", "all none"]);
    let reads = script
        .seen()
        .iter()
        .filter(|line| line.contains("/files"))
        .count();
    assert_eq!(reads, 1);
    let mut origin = event(Visibility::Public).origin().unwrap().clone();
    origin.repository = "x/y".to_owned();
    let foreign = event(Visibility::Public).from_origin(origin);
    let unread = Script::keyed(BTreeMap::new()).then(&[POSTED]);
    assert_eq!(
        announced(&config, &healthy(), &unread, &foreign),
        ["all none"]
    );
    assert!(!unread.seen().iter().any(|line| line.contains("/files")));
    let failed = Script::keyed(BTreeMap::new()).then(&[("/files?per_page=100", 500, "{}"), POSTED]);
    assert_eq!(
        announced(&config, &healthy(), &failed, &event(Visibility::Public)),
        ["all none"]
    );
}

#[test]
fn a_private_repository_s_areas_reach_only_the_routes_said_private() {
    let config = config("");
    let store = healthy();
    kept(&store, "c0ffee", &["web/index.html"], true);
    let script = posting();
    assert_eq!(
        announced(&config, &store, &script, &event(Visibility::Private)),
        ["web web"]
    );
    let posted: Vec<String> = script
        .bodies()
        .into_iter()
        .map(|body| String::from_utf8(body).unwrap())
        .collect();
    assert_eq!(posted.len(), 1, "{posted:?}");
}

use std::collections::BTreeSet;

use herald_config::{Config, KeyCache, Uses, load};
use herald_flows::{
    Cadence, Egress, Findings, Gap, Here, Layer, Memory, Outcome, Ports, Rounds, Stop, Tier,
    doctor, online, prepare, secrets, trial,
};
use herald_testing::FakeClock;
use herald_testing::{FakeSecrets, FakeSigner};
use jiff::SignedDuration;

use crate::harness::*;

fn loaded(text: &str) -> Config {
    load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

const STORES: [&str; 3] = ["sqlite", "markers", "repo-variables"];

fn actions() -> Layer<'static> {
    Layer {
        name: "actions",
        tier: Tier::Actions,
        stores: &STORES,
        endpoint: false,
        sessions: false,
        egress: Egress::Checked,
        rounds: Rounds::Workflow,
        round_secrets: &[],
        keyring: Memory::PerRequest,
    }
}

fn fastly() -> Layer<'static> {
    Layer {
        name: "fastly",
        tier: Tier::Serverless,
        endpoint: true,
        egress: Egress::Unchecked,
        rounds: Rounds::Asked,
        round_secrets: &["HERALD_ROUND_SECRET"],
        ..actions()
    }
}

fn worker() -> Layer<'static> {
    Layer {
        name: "worker",
        tier: Tier::Serverless,
        endpoint: true,
        sessions: true,
        rounds: Rounds::Platform(Cadence {
            by: "the cron triggers",
            poll: SignedDuration::from_mins(5),
            housekeep: SignedDuration::from_hours(1),
        }),
        ..actions()
    }
}

fn server() -> Layer<'static> {
    Layer {
        name: "server",
        tier: Tier::Container,
        endpoint: true,
        sessions: true,
        rounds: Rounds::Scheduled,
        keyring: Memory::Lasting,
        ..actions()
    }
}

fn layers() -> [Layer<'static>; 4] {
    [actions(), fastly(), worker(), server()]
}

fn on(config: &Config, layer: Layer<'_>, here: &Here) -> Findings {
    doctor(config, layer, &layers(), here)
}

fn keyed(findings: &Findings) -> Vec<(&str, Option<Tier>)> {
    findings
        .gaps
        .iter()
        .map(|gap| (gap.key.as_str(), gap.first_layer))
        .collect()
}

fn served() -> Config {
    loaded(&crate::serving::config_text())
}

#[test]
fn a_layer_with_no_endpoint_names_the_actor_webhooks_and_apps_it_cannot_serve() {
    let findings = on(&served(), actions(), &Here::default());
    assert_eq!(
        keyed(&findings),
        [
            ("destinations.fedi", Some(Tier::Serverless)),
            ("sources.gate.webhook", Some(Tier::Serverless)),
            ("apps.ci.tokens", Some(Tier::Serverless)),
        ]
    );
    assert_eq!(
        findings.gaps[0],
        Gap {
            feature: "herald's own ActivityPub actor".to_owned(),
            key: "destinations.fedi".to_owned(),
            why: "it is served at herald's endpoint, which this layer has none of".to_owned(),
            first_layer: Some(Tier::Serverless),
        }
    );
    assert!(
        findings.gaps[1]
            .why
            .ends_with("; announce the run's own event, or poll the source")
    );
    assert!(
        findings.gaps[2]
            .why
            .ends_with("; announce the app's events with herald announce --event")
    );
    assert_eq!(keyed(&on(&served(), server(), &Here::default())), []);
}

#[test]
fn an_actor_on_a_host_that_cannot_check_addresses_needs_its_egress_guarded() {
    let unset = on(&served(), fastly(), &Here::default());
    assert_eq!(keyed(&unset), []);
    let egress = "destinations.fedi: herald's own ActivityPub actor is served only where its setting is made: the fastly layer resolves names out of herald's sight, so the actor reads no key and delivers to no inbox until HERALD_EGRESS_GUARDED is true; set it once the host's egress refuses private, loopback and link-local addresses.";
    assert!(
        unset.notes.iter().any(|note| note == egress),
        "{:?}",
        unset.notes
    );
    let online = Here {
        online: true,
        ..Here::default()
    };
    assert_eq!(
        keyed(&on(&served(), fastly(), &online)),
        [
            ("destinations.fedi", Some(Tier::Serverless)),
            ("HERALD_ROUND_SECRET", Some(Tier::Actions)),
        ]
    );
    let guarded = Here {
        online: true,
        guarded: true,
        round_secrets: BTreeSet::from(["HERALD_ROUND_SECRET".to_owned()]),
    };
    let set = on(&served(), fastly(), &guarded);
    assert_eq!(keyed(&set), []);
    assert!(
        !set.notes
            .iter()
            .any(|note| note.contains("is served only where"))
    );
}

#[test]
fn keys_kept_in_memory_are_noted_where_the_layer_drops_them_after_each_request() {
    let dropped = "[server.keys] cache is \"memory\", which the worker layer drops after each request, so every inbound activity has its signer's key read again; set cache = \"store\" to keep keys across requests.";
    let noted = |config: &Config, layer: Layer<'_>| {
        on(config, layer, &Here::default())
            .notes
            .iter()
            .any(|note| note == dropped)
    };
    assert!(noted(&served(), worker()));
    assert!(!noted(&served(), server()));
    let mut stored = served();
    stored.server.keys = KeyCache::Store;
    assert!(!noted(&stored, worker()));
    assert!(!noted(&config(""), worker()));
}

#[test]
fn the_markers_store_cannot_refuse_a_replayed_webhook_and_gitlab_may_not_sign() {
    let marked = loaded(
        r#"version = 1
[store]
kind = "markers"
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.gate]
kind = "gitlab"
repository = "group/project"
webhook = { secret = "HERALD_WEBHOOK" }
[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#,
    );
    let findings = on(&marked, server(), &Here::default());
    let replayed = findings
        .gaps
        .iter()
        .find(|gap| gap.feature == "refusing a replayed webhook")
        .unwrap();
    assert_eq!(replayed.key, "sources.gate.webhook");
    assert_eq!(replayed.first_layer, None);
    assert_eq!(
        replayed.why,
        "the markers store keeps no deliveries, so a replay reaches the parser and only the ledger skips it; choose a store that keeps records"
    );
    assert!(
        findings
            .notes
            .iter()
            .any(|note| note.starts_with("The source gate: GitLab signs a webhook only")),
        "{:?}",
        findings.notes
    );
    let unsigned = on(&served(), server(), &Here::default());
    assert!(
        !unsigned
            .notes
            .iter()
            .any(|note| note.starts_with("The source"))
    );
    assert!(
        !unsigned
            .gaps
            .iter()
            .any(|gap| gap.feature == "refusing a replayed webhook")
    );
}

#[test]
fn each_layer_says_what_asks_it_for_rounds_and_where_the_schedule_disagrees() {
    let schedule = |extra: &str| config(&format!("[schedule]\n{extra}"));
    let notes = |config: &Config, layer: Layer<'_>| on(config, layer, &Here::default()).notes;
    let unset = notes(&config(""), worker());
    assert!(unset.contains(
        &"On the worker layer, the cron triggers asks for rounds: a poll every 5m and housekeeping every 1h, as shipped.".to_owned()
    ), "{unset:?}");
    let disagreeing = notes(&schedule("poll = \"10m\"\nhousekeep = \"2h\""), worker());
    assert!(disagreeing.contains(&"schedule.poll is 10m, while on the worker layer the cron triggers asks for a poll every 5m as shipped; set the two alike.".to_owned()));
    assert!(disagreeing.contains(&"schedule.housekeep is 2h, while on the worker layer the cron triggers asks for housekeeping every 1h as shipped; set the two alike.".to_owned()));
    let agreeing = notes(&schedule("poll = \"5m\"\nhousekeep = \"1h\""), worker());
    assert!(
        !agreeing
            .iter()
            .any(|note| note.contains("set the two alike"))
    );
    let workflow = "[schedule] sets nothing on the actions layer: a scheduled workflow runs herald poll and herald housekeep as often as its cron says.".to_owned();
    assert!(notes(&schedule("poll = \"5m\""), actions()).contains(&workflow));
    assert!(notes(&schedule("housekeep = \"1h\""), actions()).contains(&workflow));
    assert!(!notes(&config(""), actions()).contains(&workflow));
    let asked = "On the fastly layer, a round runs only when a request holding the round secret asks for it at POST /herald/round/poll or /herald/round/housekeep; have a scheduler send them";
    assert!(notes(&config(""), fastly()).contains(&format!("{asked}.")));
    assert!(
        notes(&schedule("poll = \"5m\""), fastly()).contains(&format!(
            "{asked} as often as [schedule] says, which asks for nothing there itself."
        ))
    );
    let rounded = "HERALD_ROUND_SECRET: rounds of polling and housekeeping is served only where its setting is made: on the fastly layer a round runs only when a request holding HERALD_ROUND_SECRET asks for it, and none is set.".to_owned();
    assert!(notes(&config(""), fastly()).contains(&rounded));
    let vercel = Layer {
        name: "vercel",
        round_secrets: &["HERALD_ROUND_SECRET", "CRON_SECRET"],
        ..worker()
    };
    let cron = Here {
        round_secrets: BTreeSet::from(["CRON_SECRET".to_owned()]),
        ..Here::default()
    };
    assert!(
        !on(&config(""), vercel, &cron)
            .notes
            .iter()
            .any(|note| note.starts_with("HERALD_ROUND_SECRET"))
    );
    assert!(
        on(&config(""), vercel, &Here::default())
            .notes
            .iter()
            .any(|note| note
                .contains("a request holding HERALD_ROUND_SECRET or CRON_SECRET asks for it"))
    );
    assert!(
        !notes(&schedule("poll = \"5m\""), server())
            .iter()
            .any(|note| note.contains("round"))
    );
}

#[test]
fn a_route_reached_by_webhook_is_told_what_a_bot_token_would_add() {
    let hooked = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.hook]
kind = "discord"
webhook = { secret = "HERALD_DISCORD_WEBHOOK" }
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "hooked"
sources = ["gate"]
destination = "hook"
template = "t"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#,
    );
    let notes = on(&hooked, actions(), &Here::default()).notes;
    assert!(
        notes.contains(&"The route hooked cannot crosspost, open a thread from a message, pin, edit a post, read back its posts for markers or take commands through \"hook\" as it is reached; a bot token could.".to_owned()),
        "{notes:?}"
    );
    assert!(
        !on(&config(""), actions(), &Here::default())
            .notes
            .iter()
            .any(|note| note.contains("a bot token could"))
    );
}

#[test]
fn the_job_s_own_token_keeps_no_repository_variables() {
    let variables = |token: &str| {
        let mut configured = config("");
        configured.store = loaded(&format!(
            "version = 1\n[store]\nkind = \"repo-variables\"\ntoken = {{ secret = \"{token}\" }}\nkey = {{ secret = \"K\" }}\n"
        ))
        .store;
        configured
    };
    let job = on(&variables("GITHUB_TOKEN"), actions(), &Here::default());
    assert_eq!(keyed(&job), [("store.token", None)]);
    assert_eq!(job.gaps[0].feature, "keeping records in repo-variables");
    assert!(
        job.gaps[0]
            .why
            .starts_with("a workflow's GITHUB_TOKEN cannot write")
    );
    assert_eq!(keyed(&on(&variables("T"), actions(), &Here::default())), []);
}

#[test]
fn every_secret_is_read_and_one_not_set_here_is_noted_or_a_gap_online() {
    let configured = served();
    let read = |given: &FakeSecrets, online: bool| {
        let here = Here {
            online,
            ..Here::default()
        };
        pollster::block_on(secrets(&configured, given, &here))
    };
    let none = FakeSecrets::default();
    assert_eq!(
        read(&none, false),
        Findings {
            notes: vec!["Nothing that reads HERALD_ACTIVITYPUB_KEY, HERALD_WEBHOOK is checked, as they are not set here; herald doctor --online reads every secret.".to_owned()],
            ..Findings::default()
        }
    );
    let one = FakeSecrets::default().with("HERALD_WEBHOOK", "w");
    assert_eq!(
        read(&one, false).notes,
        [
            "Nothing that reads HERALD_ACTIVITYPUB_KEY is checked, as it is not set here; herald doctor --online reads every secret."
        ]
    );
    let online = read(&one, true);
    assert_eq!(keyed(&online), [("destinations.fedi.key", None)]);
    assert_eq!(
        online.gaps[0].feature,
        "reading the secret HERALD_ACTIVITYPUB_KEY"
    );
    assert_eq!(
        online.gaps[0].why,
        "no secret is named HERALD_ACTIVITYPUB_KEY; set it where this platform keeps secrets"
    );
    let mut unkeyed = served();
    unkeyed.secret_keys.clear();
    let here = Here {
        online: true,
        ..Here::default()
    };
    let online = pollster::block_on(secrets(&unkeyed, &one, &here));
    assert_eq!(keyed(&online), [("HERALD_ACTIVITYPUB_KEY", None)]);
    let all = one.with("HERALD_ACTIVITYPUB_KEY", "k");
    assert_eq!(read(&all, true), Findings::default());
}

#[test]
fn online_each_route_s_platform_is_asked_and_its_answers_named_once_per_destination() {
    let configured = config(
        "[[routes]]\nid = \"again\"\nsources = [\"gate\"]\ndestination = \"discord\"\nchannel = \"1566200000000000101\"\ntemplate = \"release\"\n",
    );
    let prepared = pollster::block_on(prepare(
        &configured,
        &crate::harness::secrets(),
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let ask = |answers: &[(&str, u16, &str)]| {
        let script = Script::answering(answers);
        let connected = prepared
            .clone()
            .connect(|_| Ok::<_, ()>(script.clone()))
            .unwrap();
        (pollster::block_on(online(&connected)), script.seen().len())
    };
    let (taken, asked) = ask(&[
        ("/users/@me", 200, "{}"),
        ("/channels/1566200000000000100", 200, "{}"),
        ("/users/@me", 200, "{}"),
        ("/channels/1566200000000000101", 404, "{}"),
    ]);
    assert_eq!(asked, 4);
    assert_eq!(keyed(&taken), [("routes[1].channel", None)]);
    assert_eq!(taken.gaps[0].feature, "announcing on the route again");
    assert_eq!(
        taken.notes,
        ["The destination \"discord\" takes its credential."]
    );
    let (refused, _) = ask(&[("/users/@me", 401, "{}"), ("/users/@me", 401, "{}")]);
    assert_eq!(keyed(&refused), [("destinations.discord", None)]);
    assert_eq!(
        refused.gaps[0].feature,
        "posting through the destination \"discord\""
    );
    assert_eq!(
        refused.gaps[0].why,
        "Discord refuses its credential, answering 401"
    );
    assert_eq!(refused.notes, Vec::<String>::new());
}

#[test]
fn findings_extend_in_order() {
    let gap = |key: &str| Gap {
        feature: String::new(),
        key: key.to_owned(),
        why: String::new(),
        first_layer: None,
    };
    let mut findings = Findings {
        gaps: vec![gap("a")],
        notes: vec!["one".to_owned()],
        mistakes: vec!["first".to_owned()],
    };
    findings.extend(Findings {
        gaps: vec![gap("b")],
        notes: vec!["two".to_owned()],
        mistakes: vec!["second".to_owned()],
    });
    assert_eq!(keyed(&findings), [("a", None), ("b", None)]);
    assert_eq!(findings.notes, ["one", "two"]);
    assert_eq!(findings.mistakes, ["first", "second"]);
}

#[test]
fn online_what_a_platform_says_is_noted_and_what_it_withholds_from_a_route_is_a_gap() {
    let configured = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.social]
kind = "mastodon"
server = "https://social.example"
token = { secret = "T" }
[destinations.chat]
kind = "slack"
token = { secret = "T" }
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "tooted"
sources = ["gate"]
destination = "social"
template = "t"
[[routes]]
id = "chatted"
sources = ["gate"]
destination = "chat"
channel = "C0123456789"
template = "t"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#,
    );
    let given = FakeSecrets::default().with("T", "token");
    let prepared =
        pollster::block_on(prepare(&configured, &given, &FakeSigner, Uses::Polls)).unwrap();
    let script = Script::answering(&[
        ("/api/v1/accounts/verify_credentials", 200, "{}"),
        (
            "/api/v2/instance",
            200,
            r#"{"configuration":{"statuses":{"max_characters":1000,"characters_reserved_per_url":23}}}"#,
        ),
        ("/auth.test", 200, r#"{"ok":true}"#),
    ])
    .headed(&[("x-oauth-scopes", "chat:write")]);
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let findings = pollster::block_on(online(&connected));
    assert_eq!(keyed(&findings), [("routes[1]", None)]);
    assert_eq!(findings.gaps[0].feature, "announcing on the route chatted");
    assert_eq!(
        findings.notes,
        [
            "The route tooted: the server takes 1000 characters in a status, more than the 500 herald writes; set limit = 1000 to use them",
            "The destination \"chat\" takes its credential.",
            "The destination \"social\" takes its credential.",
        ]
    );
}

fn tried(configured: &Config, script: &Script) -> Vec<herald_flows::Announced> {
    let given = crate::harness::secrets().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let prepared =
        pollster::block_on(prepare(configured, &given, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let store = healthy();
    let ports = Ports {
        store: &store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
    };
    pollster::block_on(trial(configured, &connected, &ports, "doctor"))
}

#[test]
fn a_test_message_is_a_plain_post_worded_by_its_destination() {
    let script = Script::answering(&[POSTED]);
    let posted = tried(&config(""), &script);
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].route.as_str(), "releases");
    assert_eq!(
        posted[0].outcome,
        Outcome::Sent {
            steps: vec![herald_core::Step::Post]
        }
    );
    assert_eq!(script.seen().len(), 1);
    let body = String::from_utf8(script.bodies()[0].clone()).unwrap();
    assert!(body.contains("herald test message"), "{body}");
    assert!(body.contains("the route releases reaches here"), "{body}");
    let worded = config(
        "[destinations.discord.interactions]\napplication = \"1566184281406177000\"\nkey = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nwording = { test_title = \"Proef\", test_body = \"Route {{ route }}\" }\n",
    );
    let script = Script::answering(&[POSTED]);
    tried(&worded, &script);
    let body = String::from_utf8(script.bodies()[0].clone()).unwrap();
    assert!(
        body.contains("Proef") && body.contains("Route releases"),
        "{body}"
    );
    assert_eq!(tried(&served(), &Script::default()), Vec::new());
}

#[test]
fn a_test_message_with_no_title_is_not_posted() {
    let blank = config(
        "[destinations.discord.interactions]\napplication = \"1566184281406177000\"\nkey = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nwording = { test_title = \" \" }\n",
    );
    let script = Script::default();
    let posted = tried(&blank, &script);
    assert_eq!(
        posted[0].outcome,
        Outcome::Stopped {
            landed: Vec::new(),
            remaining: Vec::new(),
            reason: Stop::Template("the event has no title".to_owned()),
        }
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

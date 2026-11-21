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

pub(crate) fn actions() -> Layer {
    Layer {
        name: "actions",
        tier: Tier::Actions,
        stores: STORES.to_vec(),
        runs: Vec::new(),
        endpoint: false,
        sessions: false,
        egress: Egress::Checked,
        rounds: Rounds::Workflow,
        round_secrets: &[],
        keyring: Memory::PerRequest,
    }
}

fn fastly() -> Layer {
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

fn worker() -> Layer {
    Layer {
        name: "worker",
        tier: Tier::Serverless,
        runs: vec![herald_models::Speaks::WorkersAi],
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

fn server() -> Layer {
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

fn layers() -> [Layer; 4] {
    [actions(), fastly(), worker(), server()]
}

fn on(config: &Config, layer: &Layer, here: &Here) -> Findings {
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
    let findings = on(&served(), &actions(), &Here::default());
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
    assert_eq!(keyed(&on(&served(), &server(), &Here::default())), []);
}

#[test]
fn an_owner_registering_member_webhooks_needs_herald_s_address() {
    let mut hooking = crate::owner::owned_in("", |options| {
        options.member_hooks = Some(herald_sources_forge::MemberHooks::Setup);
    });
    let unaddressed = "The source gate: member_hooks points each project's webhook at [server] url, which the config gives none of; give it, or run herald hooks gate --url <herald's address>";
    let noted = |config: &Config| {
        on(config, &server(), &Here::default())
            .notes
            .iter()
            .any(|note| note == unaddressed)
    };
    assert!(noted(&hooking));
    hooking.server.url = Some("https://herald.example".to_owned());
    assert!(!noted(&hooking));
    assert!(!noted(&crate::owner::owned_in("", |_| ())));
}

#[test]
fn an_actor_on_a_host_that_cannot_check_addresses_needs_its_egress_guarded() {
    let unset = on(&served(), &fastly(), &Here::default());
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
        keyed(&on(&served(), &fastly(), &online)),
        [
            ("destinations.fedi", Some(Tier::Serverless)),
            ("HERALD_ROUND_SECRET", Some(Tier::Actions)),
        ]
    );
    let guarded = Here {
        online: true,
        guarded: true,
        round_secrets: BTreeSet::from(["HERALD_ROUND_SECRET".to_owned()]),
        models: false,
    };
    let set = on(&served(), &fastly(), &guarded);
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
    let noted = |config: &Config, layer: Layer| {
        on(config, &layer, &Here::default())
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
    let findings = on(&marked, &server(), &Here::default());
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
    let unsigned = on(&served(), &server(), &Here::default());
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
    let notes = |config: &Config, layer: Layer| on(config, &layer, &Here::default()).notes;
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
    let asked = "On the fastly layer, a round runs only when a request holding the round secret asks for it at POST /herald/round/poll, /herald/round/report or /herald/round/housekeep; have a scheduler send them";
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
        !on(&config(""), &vercel, &cron)
            .notes
            .iter()
            .any(|note| note.starts_with("HERALD_ROUND_SECRET"))
    );
    assert!(
        on(&config(""), &vercel, &Here::default())
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
    let notes = on(&hooked, &actions(), &Here::default()).notes;
    assert!(
        notes.contains(&"The route hooked cannot crosspost, open a thread from a message, pin, read back its posts for markers or take commands through \"hook\" as it is reached; a bot token could.".to_owned()),
        "{notes:?}"
    );
    assert!(
        !on(&config(""), &actions(), &Here::default())
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
    let job = on(&variables("GITHUB_TOKEN"), &actions(), &Here::default());
    assert_eq!(keyed(&job), [("store.token", None)]);
    assert_eq!(job.gaps[0].feature, "keeping records in repo-variables");
    assert!(
        job.gaps[0]
            .why
            .starts_with("a workflow's GITHUB_TOKEN cannot write")
    );
    assert_eq!(
        keyed(&on(&variables("T"), &actions(), &Here::default())),
        []
    );
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
        (
            pollster::block_on(online(&connected, (&FakeSigner, crate::harness::NOON))),
            script.seen().len(),
        )
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
        [
            "The route releases: herald withholds advisories sections from this channel: Discord's answer did not say who may view the channel",
            "The destination \"discord\" takes its credential."
        ]
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
    let findings = pollster::block_on(online(&connected, (&FakeSigner, crate::harness::NOON)));
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
        budget: herald_flows::Budget::UNBOUNDED,
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

fn listed_online(config: &Config, script: &Script) -> Findings {
    let prepared = pollster::block_on(prepare(
        config,
        &crate::harness::secrets(),
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let mut findings = pollster::block_on(online(&connected, (&FakeSigner, crate::harness::NOON)));
    findings
        .notes
        .retain(|note| !note.starts_with("The destination"));
    findings
}

#[test]
fn an_owner_is_listed_online_and_members_sharing_a_project_name_or_passing_a_cap_are_mistakes() {
    use crate::owner::{LISTING, owned_in, repositories};
    let config = owned_in("", |_| {});
    let probe = |answers: &[(&str, u16, &str)]| {
        let mut all = vec![
            ("/users/@me", 200, r#"{"id":"99"}"#),
            (
                "/channels/1566200000000000100",
                200,
                r#"{"id":"1566200000000000100","guild_id":"1","type":5}"#,
            ),
        ];
        all.extend_from_slice(answers);
        Script::answering(&all)
    };
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let findings = listed_online(&config, &probe(&[(LISTING, 200, &two)]));
    assert_eq!(findings.mistakes, Vec::<String>::new());
    assert!(
        findings
            .notes
            .contains(&"The source gate takes 2 repositories.".to_owned())
    );
    let twins = repositories(&[(1, "cli"), (2, "CLI")]);
    let findings = listed_online(&config, &probe(&[(LISTING, 200, &twins)]));
    assert_eq!(
        findings.mistakes,
        [
            "gate's nightworksio/cli, gate's nightworksio/CLI are all announced as the project cli; leave all but one out with exclude, or name a narrower owner."
        ]
    );
    let capped = owned_in("", |options| {
        options.most_members = std::num::NonZeroUsize::new(1);
    });
    let findings = listed_online(&capped, &probe(&[(LISTING, 200, &two)]));
    assert_eq!(
        findings.mistakes,
        [
            "The source gate: 2 repositories of nightworksio are taken, more than most_members, 1; raise it, or narrow the filters."
        ]
    );
    let mut filed = owned_in("", |_| {});
    if let Some(source) = filed.sources.values_mut().next()
        && let Some(mut forged) = source.of::<herald_sources_forge::ForgeSource>().cloned()
    {
        forged.names = Some(vec!["herald".to_owned(), "gone".to_owned()]);
        *source = herald_config::Source::new(forged);
    }
    let findings = listed_online(&filed, &probe(&[(LISTING, 200, &two)]));
    assert_eq!(
        findings.mistakes,
        [
            "The source gate's member file names gone, which its owner does not hold; correct or remove them."
        ]
    );
    assert!(
        findings
            .notes
            .contains(&"The source gate takes 1 repository.".to_owned())
    );
    let renamed = owned_in("", |options| {
        options.project = Some("{{ repository.owner }}".to_owned());
    });
    let findings = listed_online(&renamed, &probe(&[(LISTING, 200, &two)]));
    assert_eq!(
        findings.mistakes,
        [
            "gate's nightworksio/herald, gate's nightworksio/gate are all announced as the project nightworksio; leave all but one out with exclude, or name a narrower owner."
        ]
    );
    let unnamed = owned_in("", |options| {
        options.project = Some("{{ repository.properties.team[0] }}".to_owned());
    });
    let findings = listed_online(&unnamed, &probe(&[(LISTING, 200, &two)]));
    assert_eq!(findings.mistakes.len(), 2);
    assert!(
        findings.mistakes[0].starts_with(
            "The source gate names no project for nightworksio/herald: template project: "
        ),
        "{}",
        findings.mistakes[0]
    );
    let findings = listed_online(&config, &probe(&[(LISTING, 500, "{}")]));
    assert_eq!(keyed(&findings), [("sources.gate.owner", None)]);
    assert_eq!(
        findings.gaps[0].feature,
        "listing the owner of the source \"gate\""
    );
}

/// A config whose store is `store`, its destination `destination`, its
/// source announcing `signals`, and its route set as `route`.
fn revising(store: &str, destination: &str, signals: &str, route: &str) -> Config {
    let prefer = if signals.contains("tag") && signals.contains("release") {
        "prefer = \"first\""
    } else {
        ""
    };
    loaded(&format!(
        r#"version = 1
[store]
kind = "{store}"
[destinations.out]
{destination}
[sources.gate]
kind = "github"
repository = "o/r"
signals = {signals}
{prefer}
[[routes]]
id = "out"
sources = ["gate"]
destination = "out"
template = "t"
{route}
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#
    ))
}

#[test]
fn a_route_that_cannot_edit_is_told_what_it_does_with_a_late_or_edited_release() {
    const HOOK: &str = "kind = \"slack\"\nwebhook = { secret = \"HERALD_SLACK_WEBHOOK\" }";
    const BOT: &str = "kind = \"slack\"\ntoken = { secret = \"HERALD_SLACK_TOKEN\" }";
    const BOTH: &str = "[\"release\", \"tag\"]";
    let noted = |config: &Config, said: &str| {
        on(config, &actions(), &Here::default())
            .notes
            .iter()
            .any(|note| note.starts_with("The route out") && note.contains(said))
    };
    let hooked = revising("sqlite", HOOK, BOTH, "edits = \"release\"");
    assert!(noted(
        &hooked,
        "posts a Release whose tag it announced anew"
    ));
    assert!(noted(
        &hooked,
        "leaves an edited Release as first announced"
    ));
    let quiet = revising("sqlite", HOOK, BOTH, "late_release = \"skip\"");
    assert!(!noted(&quiet, "anew"));
    assert!(!noted(&quiet, "edited"));
    let released = revising("sqlite", HOOK, "[\"release\"]", "");
    assert!(!noted(&released, "anew"));
    let tagged = revising("sqlite", HOOK, "[\"tag\"]", "edits = \"release\"");
    assert!(!noted(&tagged, "edited"));
    let bot = revising(
        "sqlite",
        BOT,
        BOTH,
        "channel = \"C0123456789\"\nedits = \"release\"",
    );
    assert!(!noted(&bot, "anew"));
    assert!(!noted(&bot, "edited"));
}

#[test]
fn the_markers_store_edits_no_post_and_posts_no_late_release_anew() {
    const BOT: &str = "kind = \"discord\"\ntoken = { secret = \"HERALD_DISCORD_TOKEN\" }";
    let noted = |config: &Config| {
        on(config, &actions(), &Here::default())
            .notes
            .iter()
            .any(|note| note.contains("the markers store keeps no record of what a post shows"))
    };
    let route = "channel = \"1566200000000000100\"";
    assert!(noted(&revising(
        "markers",
        BOT,
        "[\"release\", \"tag\"]",
        route
    )));
    assert!(noted(&revising(
        "markers",
        BOT,
        "[\"release\"]",
        &format!("{route}\nedits = \"release\"")
    )));
    assert!(!noted(&revising("markers", BOT, "[\"release\"]", route)));
    assert!(!noted(&revising(
        "markers",
        BOT,
        "[\"release\", \"tag\"]",
        &format!("{route}\nlate_release = \"skip\"")
    )));
}

#[test]
fn online_a_namespace_listing_a_package_outside_it_is_a_gap_at_the_namespace_key() {
    let mut config = config("");
    config.sources.insert(
        herald_core::SourceName::new("gate"),
        herald_config::Source::new(herald_sources_registry::RegistrySource {
            registry: herald_sources_registry::Registry::DockerHub,
            options: herald_sources_registry::RegistryOptions {
                namespace: Some("acme".to_owned()),
                ..herald_sources_registry::RegistryOptions::default()
            },
        }),
    );
    let own = r#"{"results":[{"name":"a","namespace":"acme","is_private":false}],"next":null}"#;
    let foreign = r#"{"results":[{"name":"b","namespace":"evil","is_private":false}],"next":null}"#;
    let answering = |listing: &'static str| {
        Script::answering(&[
            ("/users/@me", 200, r#"{"id":"99"}"#),
            (
                "/channels/1566200000000000100",
                200,
                r#"{"id":"1566200000000000100","guild_id":"1","type":5}"#,
            ),
            (
                "/namespaces/acme/repositories?page_size=100&page=1",
                200,
                listing,
            ),
        ])
    };
    let findings = listed_online(&config, &answering(own));
    assert!(
        findings
            .notes
            .contains(&"The source gate takes 1 package.".to_owned()),
        "{:?}",
        findings.notes
    );
    let findings = listed_online(&config, &answering(foreign));
    assert_eq!(keyed(&findings), [("sources.gate.namespace", None)]);
    assert_eq!(
        findings.gaps[0].feature,
        "listing the namespace of the source \"gate\""
    );
    assert!(
        findings.gaps[0]
            .why
            .contains("it names evil/b, outside the namespace acme"),
        "{:?}",
        findings.gaps[0]
    );
}

#[test]
fn an_owner_registering_member_webhooks_is_checked_online_for_its_token_and_each_member() {
    use crate::owner::{LISTING, owned_in, repositories};
    let hooked = |member_hooks| {
        let mut config = owned_in("", |options| {
            options.member_hooks = Some(member_hooks);
            options.webhook = Some(herald_core::SecretRef {
                secret: "HERALD_APP_KEY".to_owned(),
            });
        });
        config.server.url = Some("https://herald.example".to_owned());
        config
    };
    let probed = |answers: &[(&str, u16, &str)]| {
        let mut all = vec![
            ("/users/@me", 200, r#"{"id":"99"}"#),
            (
                "/channels/1566200000000000100",
                200,
                r#"{"id":"1566200000000000100","guild_id":"1","type":5}"#,
            ),
        ];
        all.extend_from_slice(answers);
        Script::answering(&all)
    };
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let found = r#"[{"url":"https://herald.example/hooks/gate"}]"#;
    let round = listed_online(
        &hooked(herald_sources_forge::MemberHooks::Round),
        &probed(&[
            (LISTING, 200, &two),
            (
                "/personal_access_tokens/self",
                200,
                r#"{"scopes":["read_api"]}"#,
            ),
            ("/projects/1/hooks?per_page=100", 200, found),
            ("/projects/2/hooks?per_page=100", 200, "[]"),
        ]),
    );
    assert!(round.mistakes.iter().any(|mistake| mistake
        == "The source gate's token cannot register its members' webhooks: its scopes are read_api, and registering a project webhook needs the api scope; give it the api scope and Maintainer access, or set member_hooks = \"setup\"."));
    assert!(round.mistakes.iter().any(|mistake| mistake
        == "The source gate's members nightworksio/gate have no webhook to herald; run herald hooks gate."));
    let setup = listed_online(
        &hooked(herald_sources_forge::MemberHooks::Setup),
        &probed(&[
            (LISTING, 200, &two),
            ("/projects/1/hooks?per_page=100", 200, found),
            ("/projects/2/hooks?per_page=100", 500, "{}"),
        ]),
    );
    assert!(
        setup
            .mistakes
            .iter()
            .all(|mistake| !mistake.contains("token"))
    );
    assert!(
        setup
            .gaps
            .iter()
            .any(|gap| gap.key == "sources.gate.member_hooks"
                && gap.feature == "nightworksio/gate's webhooks")
    );
}

mod addressed;
mod areas;
mod federation;
mod kept;
mod reports;
mod tasks;
mod unhosted;

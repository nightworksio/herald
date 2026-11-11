use herald_config::load;
use herald_flows::{Egress, Findings, Gap, Here, Layer, Memory, Rounds, Tier, doctor, unserved};

use crate::harness::*;

fn layer<'a>(stores: &'a [&'a str]) -> Layer<'a> {
    Layer {
        name: "actions",
        tier: Tier::Actions,
        stores,
        endpoint: false,
        sessions: false,
        egress: Egress::Checked,
        rounds: Rounds::Workflow,
        round_secrets: &[],
        keyring: Memory::PerRequest,
    }
}

/// The start of the note every run on the git-branch store gets.
const LISTED: &str = "Each kind of record is one directory on the branch";

/// What doctor finds of `config` on `layer` alone, but for the git-branch
/// store's own note.
fn doctored(config: &herald_config::Config, layer: Layer<'_>) -> Findings {
    let mut findings = doctor(config, layer, &[layer], &Here::default());
    findings.notes.retain(|note| !note.starts_with(LISTED));
    findings
}

const STORES: [&str; 2] = ["git-branch", "markers"];
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sealed(store: &str) -> &'static str {
    if store == "markers" {
        ""
    } else {
        "key = { secret = \"HERALD_STORE_KEY\" }\n"
    }
}

fn on(store: &str, extra: &str) -> herald_config::Config {
    let mut config = config(extra);
    config.store = load(
        "herald.toml",
        &format!(
            "version = 1\n[store]\nkind = \"{store}\"\n{}",
            sealed(store)
        ),
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
    .store;
    config
}

fn keys(gaps: &[Gap]) -> Vec<&str> {
    gaps.iter().map(|gap| gap.key.as_str()).collect()
}

#[test]
fn a_config_on_a_store_that_keeps_records_has_no_gaps_and_notes_a_derived_marker_key() {
    let branch = on("git-branch", "");
    let noted = doctor(&branch, layer(&STORES), &[], &Here::default()).notes;
    assert!(noted[1].starts_with(LISTED), "{noted:?}");
    let findings = doctored(&branch, layer(&STORES));
    assert_eq!(findings.gaps, []);
    assert_eq!(findings.notes.len(), 1);
    assert!(findings.notes[0].contains("Rotating that credential forgets every earlier marker"));
    let keyed = on(
        "git-branch",
        "[markers]\nkey = { secret = \"HERALD_MARKER_SECRET\" }\n",
    );
    assert_eq!(doctored(&keyed, layer(&STORES)).notes, Vec::<String>::new());
}

#[test]
fn the_actions_cache_and_repository_variables_note_what_they_cannot_hold() {
    const ALL: [&str; 4] = ["git-branch", "markers", "actions-cache", "repo-variables"];
    let cache = doctored(&on("actions-cache", ""), layer(&ALL));
    assert_eq!(cache.gaps, []);
    assert!(cache.notes[1].starts_with("The Actions cache keeps records apart for each ref"));
    let mut variables = on("git-branch", "");
    variables.store = load(
        "herald.toml",
        "version = 1\n[store]\nkind = \"repo-variables\"\ntoken = { secret = \"T\" }\nkey = { secret = \"K\" }\n",
        &|_| Err(String::new()), &crate::harness::kinds(),
    )
    .unwrap()
    .store;
    let noted = doctored(&variables, layer(&ALL));
    assert!(noted.notes[1].starts_with("Each record is a repository variable"));
    let filed = doctored(&on("sqlite", ""), layer(&ALL));
    assert_eq!(
        filed.notes[1],
        "Every record is kept in one SQLite file, so run herald where that file outlives the run: a CI runner's disk is gone when its job ends."
    );
}

#[test]
fn a_store_this_herald_does_not_serve_is_a_gap() {
    let findings = doctored(&on("sqlite", ""), layer(&STORES));
    assert_eq!(
        findings.gaps,
        [Gap {
            feature: "keeping records in sqlite".to_owned(),
            key: "store.kind".to_owned(),
            why: unserved(&STORES),
            first_layer: None,
        }]
    );
    assert_eq!(
        unserved(&STORES),
        "this herald keeps records in git-branch or markers; set [store] kind to one of them"
    );
    assert_eq!(
        unserved(&["a", "b", "c"]),
        "this herald keeps records in a, b or c; set [store] kind to one of them"
    );
    assert_eq!(
        unserved(&["a"]),
        "this herald keeps records in a; set [store] kind to one of them"
    );
}

#[test]
fn the_markers_store_names_the_pins_cursors_and_read_backs_it_lacks() {
    let mut config = on("markers", "");
    config.routes[0].route.delivery.pin = Some(herald_core::Pinning::Store);
    let findings = doctored(&config, layer(&STORES));
    assert_eq!(keys(&findings.gaps), ["routes[0].pin"]);
    assert!(findings.notes[1].starts_with("`herald poll` stops on the markers store"));
    assert!(findings.notes[2].starts_with("The markers store has no compare-and-set"));
    let mut scheduled = on("markers", "[schedule]\npoll = \"1h\"\n");
    scheduled.sources.clear();
    let polled = doctored(&scheduled, layer(&STORES));
    assert_eq!(keys(&polled.gaps), ["schedule.poll"]);
    assert_eq!(
        polled.gaps[0].why,
        "the markers store keeps no cursors; choose a store that does"
    );
    let sourced = doctored(&on("markers", ""), layer(&STORES));
    assert_eq!(sourced.notes.len(), 3);
    let mut bare = on("markers", "");
    bare.sources.clear();
    assert_eq!(doctored(&bare, layer(&STORES)).notes.len(), 2);
    config.routes[0].route.delivery.pin = Some(herald_core::Pinning::Markers);
    config.routes[0].profile = herald_dest_discord::profile(true, true);
    let forum = doctored(&config, layer(&STORES));
    assert_eq!(keys(&forum.gaps), ["routes[0].destination"]);
    assert!(
        forum.gaps[0]
            .why
            .contains("a forum's posts are not read back")
    );
    let mut hooked = on("markers", "");
    if let Some(destination) = hooked.destinations.values_mut().next()
        && let Some(mut options) = destination
            .of::<herald_dest_discord::DestinationOptions>()
            .cloned()
    {
        options.webhook = options.token.take();
        *destination = herald_config::Destination::new(options);
    }
    hooked.routes[0].route.delivery.pin = None;
    hooked.routes[0].route.delivery.crosspost = false;
    hooked.routes[0].route.delivery.thread = None;
    hooked.routes[0].profile = herald_dest_discord::profile(false, false);
    let webhook = doctored(&hooked, layer(&STORES));
    assert!(
        webhook.gaps[0]
            .why
            .contains("a webhook cannot read back its posts")
    );
    assert_eq!(webhook.gaps[0].feature, "sending each announcement once");
}

#[test]
fn a_route_taking_docker_hub_tags_without_versions_is_noted() {
    let text = |versions: &str| {
        format!(
            "version = 1\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"S\" }}\n[markers]\nkey = {{ secret = \"K\" }}\n[destinations.discord]\nkind = \"discord\"\ntoken = {{ secret = \"T\" }}\n[sources.caddy]\nkind = \"docker-hub\"\npackage = \"caddy\"\n[[routes]]\nid = \"images\"\nsources = [\"caddy\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"t\"\n{versions}\n[templates.t]\ntitle = \"{{{{ event.attributes.version }}}}\"\nbody = \"\"\n"
        )
    };
    let loaded = |versions: &str| {
        load(
            "herald.toml",
            &text(versions),
            &|_| Err(String::new()),
            &crate::harness::kinds(),
        )
        .unwrap()
    };
    let noted = doctored(&loaded(""), layer(&STORES));
    assert_eq!(
        noted.notes,
        [
            "The route images takes caddy's tags with no versions pattern, so a tag that moves, such as latest, is announced the first time it is pushed. Set versions = \"*.*.*\" on the route to leave such tags out."
        ]
    );
    assert_eq!(
        doctored(&loaded("versions = \"*.*.*\""), layer(&STORES)).notes,
        Vec::<String>::new()
    );
}

fn interacting(discord: &str, slack: &str, matrix: &str) -> herald_config::Config {
    let text = format!(
        r#"version = 1
[store]
kind = "git-branch"
key = {{ secret = "HERALD_STORE_KEY" }}
[destinations.discord]
kind = "discord"
token = {{ secret = "T" }}
[destinations.discord.interactions]
application = "123456789012345678"
key = "{KEY}"
via = "{discord}"
[destinations.slack]
kind = "slack"
token = {{ secret = "T" }}
[destinations.slack.interactions]
signing = {{ secret = "S" }}
app = {{ secret = "A" }}
via = "{slack}"
[destinations.matrix]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "T" }}
[destinations.matrix.interactions]
homeserver_token = {{ secret = "H" }}
appservice_token = {{ secret = "A" }}
via = "{matrix}"
[destinations.fedi]
kind = "mastodon"
server = "https://social.example"
token = {{ secret = "T" }}
[destinations.quiet]
kind = "discord"
token = {{ secret = "T" }}
[destinations.hushed]
kind = "slack"
token = {{ secret = "T" }}
[destinations.still]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "T" }}
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn gap_keys(findings: &herald_flows::Findings) -> Vec<String> {
    findings.gaps.iter().map(|gap| gap.key.clone()).collect()
}

/// The layers a test ranks gaps against: none with an endpoint, two
/// serverless ones of which one holds sessions, and one of its own.
fn ranked<'a>(stores: &'a [&'a str]) -> [Layer<'a>; 4] {
    let actions = layer(stores);
    let lambda = Layer {
        name: "lambda",
        tier: Tier::Serverless,
        endpoint: true,
        ..actions
    };
    [
        actions,
        lambda,
        Layer {
            name: "worker",
            sessions: true,
            ..lambda
        },
        Layer {
            name: "server",
            tier: Tier::Container,
            endpoint: true,
            sessions: true,
            rounds: Rounds::Scheduled,
            keyring: Memory::Lasting,
            ..actions
        },
    ]
}

#[test]
fn commands_posted_to_herald_need_an_endpoint_and_sessions_need_a_layer_that_holds_them() {
    let layers = ranked(&STORES);
    let [actions, lambda, worker, server] = layers;
    let on = |config: &herald_config::Config, layer: Layer<'_>| {
        doctor(config, layer, &layers, &Here::default())
    };
    let posted = interacting("http", "http", "appservice");
    let unserved = on(&posted, actions);
    assert_eq!(
        gap_keys(&unserved),
        [
            "destinations.discord.interactions.via",
            "destinations.matrix.interactions.via",
            "destinations.slack.interactions.via",
        ]
    );
    assert_eq!(unserved.gaps[0].feature, "the interactions Discord posts");
    assert_eq!(
        unserved.gaps[0].why,
        "they are posted to herald's endpoint, which this layer has none of; run herald where it serves one, or carry them over the gateway with via = \"gateway\" on the worker or server layer"
    );
    assert_eq!(unserved.gaps[0].first_layer, Some(Tier::Serverless));
    assert_eq!(
        unserved.gaps[1].feature,
        "the transactions the homeserver pushes"
    );
    assert_eq!(unserved.gaps[2].feature, "the commands Slack posts");
    assert_eq!(
        doctored(&posted, actions).gaps[0].why,
        "they are posted to herald's endpoint, which this layer has none of; run herald where it serves one"
    );
    for layer in [lambda, worker, server] {
        assert_eq!(gap_keys(&on(&posted, layer)), Vec::<String>::new());
    }
    let held = interacting("gateway", "socket", "sync");
    let gaps = on(&held, lambda).gaps;
    assert_eq!(gaps.len(), 3);
    assert_eq!(gaps[0].feature, "a session carrying commands");
    assert_eq!(
        gaps[0].why,
        "a session is held open only on the worker or server layer; run herald there, or have the commands posted to herald's endpoint"
    );
    assert_eq!(gaps[0].first_layer, Some(Tier::Serverless));
    assert_eq!(
        doctored(&held, lambda).gaps[0].why,
        "no layer holds a session open; have the commands posted to herald's endpoint"
    );
    assert_eq!(gap_keys(&on(&held, worker)), Vec::<String>::new());
    assert_eq!(gap_keys(&on(&held, server)), Vec::<String>::new());
}

/// A change to a config that sets one bound above its default.
type Loosening = fn(&mut herald_config::Config);

#[test]
fn each_bound_set_above_its_default_is_noted_by_its_key() {
    let keyed = || {
        on(
            "git-branch",
            "[markers]\nkey = { secret = \"HERALD_MARKER_SECRET\" }\n",
        )
    };
    let loosen: [(&str, Loosening); 9] = [
        ("server.body", |config| config.server.body += 1),
        ("server.inbound.replay_window", |config| {
            config.server.inbound.replay_window += jiff::SignedDuration::from_secs(1);
        }),
        ("server.inbound.signatures", |config| {
            config.server.inbound.signatures += 1;
        }),
        ("server.inbound.covered", |config| {
            config.server.inbound.covered += 1;
        }),
        ("server.keys.document", |config| {
            config.server.inbound.document += 1;
        }),
        ("http.body", |config| config.http.body += 1),
        ("render.fuel", |config| config.render.fuel += 1),
        ("render.recursion", |config| config.render.recursion += 1),
        ("render.output", |config| config.render.output += 1),
    ];
    for (key, loosened) in loosen {
        let mut config = keyed();
        loosened(&mut config);
        let notes = doctored(&config, layer(&STORES)).notes;
        assert_eq!(notes.len(), 1, "{key}: {notes:?}");
        assert!(
            notes[0].starts_with(&format!("{key} is set above its default, so ")),
            "{notes:?}"
        );
    }
    let mut tightened = keyed();
    tightened.server.inbound.replay_window = jiff::SignedDuration::from_mins(1);
    tightened.render.output = 1024;
    assert_eq!(
        doctored(&tightened, layer(&STORES)).notes,
        Vec::<String>::new()
    );
}

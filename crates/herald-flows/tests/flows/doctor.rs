use herald_config::load;
use herald_flows::{Gap, Layer, doctor, unserved};

use crate::harness::*;

fn layer<'a>(stores: &'a [&'a str]) -> Layer<'a> {
    Layer {
        stores,
        endpoint: false,
        sessions: false,
    }
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
    let findings = doctor(&on("git-branch", ""), layer(&STORES));
    assert_eq!(findings.gaps, []);
    assert_eq!(findings.notes.len(), 1);
    assert!(findings.notes[0].contains("Rotating that credential forgets every earlier marker"));
    let keyed = on(
        "git-branch",
        "[markers]\nkey = { secret = \"HERALD_MARKER_SECRET\" }\n",
    );
    assert_eq!(doctor(&keyed, layer(&STORES)).notes, Vec::<String>::new());
}

#[test]
fn the_actions_cache_and_repository_variables_note_what_they_cannot_hold() {
    const ALL: [&str; 4] = ["git-branch", "markers", "actions-cache", "repo-variables"];
    let cache = doctor(&on("actions-cache", ""), layer(&ALL));
    assert_eq!(cache.gaps, []);
    assert!(cache.notes[1].starts_with("The Actions cache keeps records apart for each ref"));
    let mut variables = on("git-branch", "");
    variables.store = load(
        "herald.toml",
        "version = 1\n[store]\nkind = \"repo-variables\"\ntoken = { secret = \"T\" }\nkey = { secret = \"K\" }\n",
        &|_| Err(String::new()),
    )
    .unwrap()
    .store;
    let noted = doctor(&variables, layer(&ALL));
    assert!(noted.notes[1].starts_with("Each record is a repository variable"));
    let filed = doctor(&on("sqlite", ""), layer(&ALL));
    assert_eq!(
        filed.notes[1],
        "Every record is kept in one SQLite file, so run herald where that file outlives the run: a CI runner's disk is gone when its job ends."
    );
}

#[test]
fn a_store_this_herald_does_not_serve_is_a_gap() {
    let findings = doctor(&on("sqlite", ""), layer(&STORES));
    assert_eq!(
        findings.gaps,
        [Gap {
            feature: "keeping records in sqlite".to_owned(),
            key: "store.kind".to_owned(),
            why: unserved(&STORES),
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
    let findings = doctor(&config, layer(&STORES));
    assert_eq!(keys(&findings.gaps), ["routes[0].pin"]);
    assert!(findings.notes[1].starts_with("`herald poll` stops on the markers store"));
    assert!(findings.notes[2].starts_with("The markers store has no compare-and-set"));
    let mut scheduled = on("markers", "[schedule]\npoll = \"1h\"\n");
    scheduled.sources.clear();
    let polled = doctor(&scheduled, layer(&STORES));
    assert_eq!(keys(&polled.gaps), ["schedule.poll"]);
    assert_eq!(
        polled.gaps[0].why,
        "the markers store keeps no cursors; choose a store that does"
    );
    let sourced = doctor(&on("markers", ""), layer(&STORES));
    assert_eq!(sourced.notes.len(), 3);
    let mut bare = on("markers", "");
    bare.sources.clear();
    assert_eq!(doctor(&bare, layer(&STORES)).notes.len(), 2);
    config.routes[0].route.delivery.pin = Some(herald_core::Pinning::Markers);
    config.routes[0].profile = herald_dest_discord::profile(true, true);
    let forum = doctor(&config, layer(&STORES));
    assert_eq!(keys(&forum.gaps), ["routes[0].destination"]);
    assert!(
        forum.gaps[0]
            .why
            .contains("a forum's posts are not read back")
    );
    let mut hooked = on("markers", "");
    if let Some(herald_config::Destination::Discord(options)) =
        hooked.destinations.values_mut().next()
    {
        options.webhook = options.token.take();
    }
    hooked.routes[0].route.delivery.pin = None;
    hooked.routes[0].route.delivery.crosspost = false;
    hooked.routes[0].route.delivery.thread = None;
    hooked.routes[0].profile = herald_dest_discord::profile(false, false);
    let webhook = doctor(&hooked, layer(&STORES));
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
            "version = 1\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"S\" }}\n[markers]\nkey = {{ secret = \"K\" }}\n[destinations.discord]\nkind = \"discord\"\ntoken = {{ secret = \"T\" }}\n[sources.caddy]\nkind = \"docker-hub\"\npackage = \"caddy\"\n[[routes]]\nid = \"images\"\nsources = [\"caddy\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"t\"\n{versions}\n[templates.t]\ntitle = \"{{{{ event.version }}}}\"\nbody = \"\"\n"
        )
    };
    let loaded =
        |versions: &str| load("herald.toml", &text(versions), &|_| Err(String::new())).unwrap();
    let noted = doctor(&loaded(""), layer(&STORES));
    assert_eq!(
        noted.notes,
        [
            "The route images takes caddy's tags with no versions pattern, so a tag that moves, such as latest, is announced the first time it is pushed. Set versions = \"*.*.*\" on the route to leave such tags out."
        ]
    );
    assert_eq!(
        doctor(&loaded("versions = \"*.*.*\""), layer(&STORES)).notes,
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
    load("herald.toml", &text, &|_| Err(String::new())).unwrap()
}

fn gap_keys(findings: &herald_flows::Findings) -> Vec<String> {
    findings.gaps.iter().map(|gap| gap.key.clone()).collect()
}

#[test]
fn commands_posted_to_herald_need_an_endpoint_and_sessions_need_herald_server() {
    let posted = interacting("http", "http", "appservice");
    let actions = doctor(&posted, layer(&STORES));
    assert_eq!(
        gap_keys(&actions),
        [
            "destinations.discord.interactions.via",
            "destinations.matrix.interactions.via",
            "destinations.slack.interactions.via",
        ]
    );
    assert_eq!(actions.gaps[0].feature, "the interactions Discord posts");
    assert_eq!(
        actions.gaps[0].why,
        "they are posted to herald's endpoint, which herald server and the serverless drivers serve; run one of them, or carry them over the gateway with via = \"gateway\" in herald server"
    );
    assert_eq!(
        actions.gaps[1].feature,
        "the transactions the homeserver pushes"
    );
    assert_eq!(actions.gaps[2].feature, "the commands Slack posts");
    let served = Layer {
        stores: &STORES,
        endpoint: true,
        sessions: true,
    };
    assert_eq!(gap_keys(&doctor(&posted, served)), Vec::<String>::new());
    let held = interacting("gateway", "socket", "sync");
    let serverless = Layer {
        endpoint: true,
        ..layer(&STORES)
    };
    let gaps = doctor(&held, serverless).gaps;
    assert_eq!(gaps.len(), 3);
    assert_eq!(gaps[0].feature, "a session carrying commands");
    assert_eq!(
        gaps[0].why,
        "only herald server holds a session open; run it, or have the commands posted to herald's endpoint"
    );
    assert_eq!(gap_keys(&doctor(&held, served)), Vec::<String>::new());
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
    let loosen: [(&str, Loosening); 8] = [
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
        ("http.body", |config| config.http.body += 1),
        ("render.fuel", |config| config.render.fuel += 1),
        ("render.recursion", |config| config.render.recursion += 1),
        ("render.output", |config| config.render.output += 1),
    ];
    for (key, loosened) in loosen {
        let mut config = keyed();
        loosened(&mut config);
        let notes = doctor(&config, layer(&STORES)).notes;
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
        doctor(&tightened, layer(&STORES)).notes,
        Vec::<String>::new()
    );
}

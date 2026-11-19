//! doctor's federation section: the instance's domain and keys, each peer,
//! feed, push and herald source, the grants and audit retention, and each
//! peer's discovery document read with `--online`.

use serde_json::{Value, json};

use super::*;

const REVOKED: [u8; 32] = [7; 32];
const PINNED: [u8; 32] = [9; 32];

fn federated() -> (Setup, World) {
    let peers = format!(
        r#"[federation]
domain = "herald.example"
keys = [{{ secret = "HERALD_FED_KEY" }}, {{ secret = "HERALD_FED_UNSET" }}]
revoked = ["{revoked}"]
[peers.org]
url = "https://herald.peer.example"
keys = ["{newer}"]
private = true
grants = [{{ resource = "sources.inbox", actions = ["push"], until = "2027-01-01T00:00:00Z" }}]
[peers.quiet]
url = "https://quiet.peer.example"
keys = ["{older}"]
[destinations.feed]
kind = "herald"
[destinations.mirror]
kind = "herald-push"
peer = "org"
inbox = "up.stream"
[destinations.quietly]
kind = "herald-push"
peer = "quiet"
inbox = "up"
[sources.inbox]
kind = "herald"
peers = {{ org = {{ actions = ["push"], until = "2027-01-01T00:00:00Z" }} }}
[sources.pulled]
kind = "herald"
peer = "org"
feed = "releases"
[sources.pinned]
kind = "herald"
url = "https://pinned.example/herald/v1/feeds/releases"
keys = ["{pinned}"]
static = true
[[routes]]
id = "fed"
sources = ["cli"]
destination = "feed"
[[routes]]
id = "pushed"
sources = ["cli"]
destination = "mirror"
[[routes]]
id = "quietly"
sources = ["cli"]
destination = "quietly"
"#,
        revoked = ed25519_public(&REVOKED),
        newer = multikey(NEWER),
        older = multikey(OLDER),
        pinned = ed25519_public(&PINNED),
    );
    let (mut setup, world) = peered(&peers, &[NEWER]);
    setup.vars.insert(
        "HERALD_FED_KEY".to_owned(),
        seeded(NEWER).expose().to_owned(),
    );
    (setup, world)
}

fn shown(key: &Key) -> Value {
    json!({ "key": ed25519_public(key), "fingerprint": fingerprint(key) })
}

#[test]
fn doctor_shows_the_instance_among_its_peers_as_json() {
    let (setup, world) = federated();
    let run = herald(&setup, &world, &["doctor", "--format", "json"]);
    let report: Value = serde_json::from_str(run.out.trim_end()).unwrap();
    let federation = &report["federation"];
    assert_eq!(federation["domain"], "herald.example", "{report}");
    assert_eq!(
        federation["keys"],
        json!([
            { "secret": "HERALD_FED_KEY", "public": shown(&public(NEWER)) },
            { "secret": "HERALD_FED_UNSET", "public": null },
        ])
    );
    assert_eq!(federation["revoked"], json!([shown(&REVOKED)]));
    assert_eq!(
        federation["peers"][0],
        json!({
            "name": "org",
            "url": "https://herald.peer.example",
            "keys": [shown(&public(NEWER))],
            "rotation": "manual",
            "rate": "120/m",
            "most": "2MiB",
            "private": true,
            "discovery": null,
        })
    );
    assert_eq!(federation["peers"][1]["private"], false);
    assert_eq!(
        federation["feeds"],
        json!([{ "name": "feed", "url": "https://herald.example/herald/v1/feeds/feed", "served": false }])
    );
    assert_eq!(
        federation["pushes"],
        json!([{
            "name": "mirror",
            "peer": "org",
            "inbox": "https://herald.peer.example/herald/v1/inbox/up.stream",
            "private": true,
        }, {
            "name": "quietly",
            "peer": "quiet",
            "inbox": "https://quiet.peer.example/herald/v1/inbox/up",
            "private": false,
        }])
    );
    assert_eq!(
        federation["sources"],
        json!([
            { "name": "inbox", "peer": null, "pinned": null, "pushed": true, "stale_after": null },
            { "name": "pinned", "peer": null, "pinned": "https://pinned.example", "pushed": false, "stale_after": 259_200 },
            { "name": "pulled", "peer": "org", "pinned": null, "pushed": false, "stale_after": null },
        ])
    );
    assert_eq!(
        federation["grants"],
        json!([{ "peer": "org", "source": "inbox", "actions": ["push"], "until": "2027-01-01T00:00:00Z" }])
    );
    assert_eq!(federation["keep_audit"], 90 * 86_400);
    assert_eq!(federation["pages"], Value::Null);
    let keys: Vec<&str> = federation["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|gap| gap["key"].as_str())
        .collect();
    assert_eq!(keys, ["federation.domain", "destinations.feed"]);
    let stored = setup_of(&config_of(&setup).replace("kind = \"git-branch\"", "kind = \"d1\""));
    let mut stored_vars = stored;
    stored_vars.vars.clone_from(&setup.vars);
    let d1 = herald(&stored_vars, &world, &["doctor", "--format", "json"]);
    let report: Value = serde_json::from_str(d1.out.trim_end()).unwrap();
    assert!(
        report["gaps"].to_string().contains("store.kind"),
        "{report}"
    );
    assert!(
        !report["federation"]["gaps"]
            .to_string()
            .contains("store.kind"),
        "{report}"
    );
    let unfederated = herald(&setup_of(CONFIG), &world, &["doctor", "--format", "json"]);
    let report: Value = serde_json::from_str(unfederated.out.trim_end()).unwrap();
    assert_eq!(report["federation"], Value::Null);
}

fn setup_of(config: &str) -> Setup {
    setup(config)
}

#[test]
fn doctor_shows_the_instance_among_its_peers_as_lines_and_reads_each_peer_online() {
    let (setup, world) = federated();
    let run = herald(&setup, &world, &["doctor"]);
    let lines: Vec<&str> = run.out.lines().collect();
    let first = lines
        .iter()
        .position(|line| line.starts_with("Federation:"))
        .unwrap_or_else(|| panic!("{}", run.out));
    let older = format!("{} {}", multikey(OLDER), fingerprint(&public(OLDER)));
    assert_eq!(
        lines[first..],
        [
            "Federation: the instance is herald.example.".to_owned(),
            format!(
                "federation.keys[0]: HERALD_FED_KEY holds {} {}.",
                multikey(NEWER),
                fingerprint(&public(NEWER))
            ),
            "federation.keys[1]: HERALD_FED_UNSET is not read here.".to_owned(),
            format!(
                "federation.revoked: {} {} is never trusted again.",
                ed25519_public(&REVOKED),
                fingerprint(&REVOKED)
            ),
            format!(
                "peers.org: https://herald.peer.example, pinning {} {}, rotation manual, rate 120/m, most 2MiB, private events go to it.",
                multikey(NEWER),
                fingerprint(&public(NEWER))
            ),
            format!(
                "peers.quiet: https://quiet.peer.example, pinning {older}, rotation manual, rate 120/m, most 2MiB, no private event goes to it."
            ),
            "destinations.feed: the feed https://herald.example/herald/v1/feeds/feed, not served on this layer.".to_owned(),
            "destinations.mirror: pushes to peers.org at https://herald.peer.example/herald/v1/inbox/up.stream, private events too.".to_owned(),
            "destinations.quietly: pushes to peers.quiet at https://quiet.peer.example/herald/v1/inbox/up, public events alone.".to_owned(),
            "sources.inbox: pulls no feed, and takes pushes.".to_owned(),
            "sources.pinned: pulls the feed of https://pinned.example from its static log, taken up to 3d after it was written, and takes no push.".to_owned(),
            "sources.pulled: pulls the feed of peers.org, and takes no push.".to_owned(),
            "federation.keep_audit: what each peer asks is kept 90d.".to_owned(),
        ]
    );
    let online = herald(&setup, &world, &["doctor", "--online"]);
    assert!(
        online
            .out
            .contains("private events go to it; its discovery document is read."),
        "{}",
        online.out
    );
    assert!(
        online
            .out
            .contains("no private event goes to it; its discovery document is not read: "),
        "{}",
        online.out
    );
    let json = herald(&setup, &world, &["doctor", "--online", "--format", "json"]);
    let report: Value = serde_json::from_str(json.out.trim_end()).unwrap();
    assert_eq!(
        report["federation"]["peers"][0]["discovery"],
        json!({ "read": true, "why": null })
    );
    assert_eq!(report["federation"]["peers"][1]["discovery"]["read"], false);
    assert!(
        report["federation"]["peers"][1]["discovery"]["why"].is_string(),
        "{report}"
    );
    let unnamed = setup_of(&format!(
        "{CONFIG}[sources.pinned]\nkind = \"herald\"\nurl = \"https://pinned.example/herald/v1/feeds/releases\"\nkeys = [\"{}\"]\n",
        ed25519_public(&PINNED)
    ));
    let lines = herald(&unnamed, &world, &["doctor"]);
    assert!(
        lines.out.contains("Federation: [federation] names no domain.\nsources.pinned: pulls the feed of https://pinned.example, and takes no push.\n"),
        "{}{}",
        lines.out, lines.err
    );
    let served = herald(&setup, &world, &["doctor", "--layer", "server"]);
    assert!(
        served.out.contains("/feeds/feed, served on this layer."),
        "{}",
        served.out
    );
}

#[test]
fn doctor_shows_where_the_static_log_is_written() {
    let mut setup = setup_of(&format!(
        "{CONFIG}\n[federation]\ndomain = \"herald.example\"\nkeys = [{{ secret = \"HERALD_FED_KEY\" }}]\n\n[federation.pages]\nurl = \"https://herald.example/log\"\nbranch = \"pages\"\nartifact = \"_site\"\n"
    ));
    setup.vars.insert(
        "HERALD_FED_KEY".to_owned(),
        seeded(NEWER).expose().to_owned(),
    );
    let json = herald(&setup, &World::default(), &["doctor", "--format", "json"]);
    let report: Value = serde_json::from_str(json.out.trim_end()).unwrap();
    assert_eq!(
        report["federation"]["pages"],
        json!({ "url": "https://herald.example/log", "branch": "pages", "artifact": "_site", "refresh": 86_400 })
    );
    let text = herald(&setup, &World::default(), &["doctor"]);
    assert!(
        text.out.contains("federation.pages: the static signed log at https://herald.example/log, committed to the branch pages and written to _site, written again unchanged after 1d."),
        "{}",
        text.out
    );
}

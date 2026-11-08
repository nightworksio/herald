use super::world::*;
use crate::*;

#[test]
fn doctor_names_the_stores_each_host_keeps_records_in() {
    let world = World::default();
    let sqlite = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\""));
    for (host, keeps) in [
        ("node", true),
        ("worker", false),
        ("deno", false),
        ("vercel", false),
        ("netlify", false),
        ("wasi", false),
        ("spin", true),
        ("fastly", false),
    ] {
        let run = herald(&sqlite, &world, &["doctor", "--layer", host]);
        assert_eq!(
            !run.out.contains("this herald keeps records in"),
            keeps,
            "{host}: {}",
            run.out
        );
    }
    let worker = herald(&sqlite, &world, &["doctor", "--layer", "worker"]);
    assert!(worker.out.contains("d1"), "{}", worker.out);
    assert!(!worker.out.contains("deno-kv"), "{}", worker.out);
    let deno = herald(&sqlite, &world, &["doctor", "--layer", "deno"]);
    assert!(deno.out.contains("deno-kv"), "{}", deno.out);
    let fastly = herald(&sqlite, &world, &["doctor", "--layer", "fastly"]);
    assert!(fastly.out.contains("fastly-kv"), "{}", fastly.out);
}

#[test]
fn doctor_names_what_the_config_leaves_to_chance() {
    let world = World::default();
    let derived = herald(&setup(CONFIG), &world, &["doctor", "--layer", "actions"]);
    assert_eq!(derived.code, DONE);
    assert!(
        derived
            .out
            .contains("reads, its names resolve and its templates render.")
    );
    assert!(
        derived
            .out
            .contains("Rotating that credential forgets every earlier marker")
    );
    let keyed = herald(
        &setup(&format!(
            "{CONFIG}[markers]\nkey = {{ secret = \"HERALD_MARKER_SECRET\" }}\n"
        )),
        &world,
        &["doctor"],
    );
    assert!(!keyed.out.contains("Rotating"), "{}", keyed.out);
    let d1 = herald(
        &setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"d1\"")),
        &world,
        &["doctor"],
    );
    assert_eq!(d1.code, STOPPED);
    assert!(
        d1.out.ends_with("store.kind: keeping records in d1 cannot be served: this herald keeps records in sqlite, postgres, dynamodb, git-branch, markers, actions-cache or repo-variables; set [store] kind to one of them.\n"),
        "{}",
        d1.out
    );
}

#[test]
fn doctor_names_a_pin_the_actions_cache_keeps_only_when_told() {
    let pinned = |keeps: &str| {
        let config = CONFIG
            .replace(
                "kind = \"git-branch\"",
                &format!("kind = \"actions-cache\"\n{keeps}"),
            )
            .replace(
                "template = \"release\"\n",
                "template = \"release\"\npin = \"store\"\n",
            );
        herald(&setup(&config), &World::default(), &["doctor"]).out
    };
    assert!(
        pinned("").contains("routes[0].pin: a pin the store remembers cannot be served: the actions-cache store keeps no pins; add \"pins\" to its keeps, or set pin = \"markers\"."),
        "{}",
        pinned("")
    );
    assert!(
        !pinned("keeps = [\"pins\"]").contains("routes[0].pin"),
        "{}",
        pinned("keeps = [\"pins\"]")
    );
}

#[test]
fn doctor_names_what_the_markers_store_cannot_serve() {
    let gaps = |config: &str| {
        let doctored = herald(&setup(config), &World::default(), &["doctor"]);
        assert_eq!(doctored.code, STOPPED, "{}", doctored.err);
        assert!(
            doctored
                .out
                .contains("The markers store has no compare-and-set")
        );
        doctored
            .out
            .lines()
            .filter(|line| line.contains("cannot be served"))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    };
    let markers = CONFIG.replace(
        "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
        MARKERS,
    );
    assert_eq!(
        gaps(&markers.replace(
            "template = \"release\"\n",
            "template = \"release\"\npin = \"store\"\n"
        )),
        [
            "routes[0].pin: a pin the store remembers cannot be served: the markers store keeps no pins; set pin = \"markers\"."
        ]
    );
    let hooked = markers.replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
        "webhook = { secret = \"HERALD_DISCORD_WEBHOOK\" }",
    );
    assert_eq!(
        gaps(&hooked),
        [
            "routes[0].destination: sending each announcement once cannot be served: the markers store reads announcements back from their posts, and a webhook cannot read back its posts; post through a bot to a text or announcement channel, or choose a store that keeps records."
        ]
    );
    let forum = markers.replace(
        "channel = \"1566200000000000100\"",
        "channel = \"1566200000000000100\"\nforum = true",
    );
    assert!(gaps(&forum)[0].contains("and a forum's posts are not read back"));
}

use super::world::*;
use crate::*;

/// What `doctor` said with `args` over `setup`, its config's path written as
/// `herald.toml`.
fn doctored(setup: &Setup, args: &[&str]) -> (u8, String) {
    let run = herald(setup, &World::default(), args);
    let path = setup.directory.path().join("herald.toml");
    (
        run.code,
        run.out.replace(&path.display().to_string(), "herald.toml"),
    )
}

/// What `doctor` says of a config with nothing to name.
const READS: &str = "herald.toml reads, its names resolve and its templates render.\n";

/// What `doctor` says of the Actions cache.
const REFS: &str = "The Actions cache keeps records apart for each ref a run is on, and drops what no run reads for a week: a release run on its tag does not see what a run on the default branch wrote. Run housekeeping and polling on one ref, or choose a store that keeps records across refs.\n";

/// What `doctor` says of the git-branch store.
const LISTED: &str = "Each kind of record is one directory on the branch, listed in one answer of GitHub's Git Trees API, which GitHub cuts short past its own limit: a directory that outgrows it can no longer be listed. Keep few records with the store's retention, or choose a database for many.\n";

/// What `doctor` says of markers keyed by a destination's credential.
const DERIVED: &str = "Each post's markers are keyed by its destination's credential, as no [markers] key is set. Rotating that credential forgets every earlier marker, so an announcement made before the rotation can be posted again on a store that has only markers. Set [markers] key = { secret = \"HERALD_MARKER_SECRET\" } to keep them.\n";

#[test]
fn doctor_names_the_stores_each_host_keeps_records_in() {
    let sqlite = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\""));
    let unserved = |kinds: &str| {
        Some(format!(
            "store.kind: keeping records in sqlite cannot be served: this herald keeps records in {kinds}; set [store] kind to one of them. The actions layer is the first that serves it."
        ))
    };
    let named: Vec<(&str, Option<String>)> = [
        "node", "worker", "deno", "vercel", "netlify", "wasi", "spin", "fastly",
    ]
    .into_iter()
    .map(|host| {
        let (_, out) = doctored(&sqlite, &["doctor", "--layer", host]);
        let line = out
            .lines()
            .find(|line| line.starts_with("store.kind: "))
            .map(ToOwned::to_owned);
        (host, line)
    })
    .collect();
    assert_eq!(
        named,
        [
            ("node", None),
            (
                "worker",
                unserved("d1, durable-object, dynamodb, git-branch, markers or repo-variables")
            ),
            (
                "deno",
                unserved("deno-kv, dynamodb, git-branch, markers or repo-variables")
            ),
            (
                "vercel",
                unserved("postgres, dynamodb, git-branch, markers or repo-variables")
            ),
            (
                "netlify",
                unserved("postgres, dynamodb, git-branch, markers or repo-variables")
            ),
            (
                "wasi",
                unserved("dynamodb, git-branch, markers or repo-variables")
            ),
            ("spin", None),
            (
                "fastly",
                unserved("fastly-kv, dynamodb, git-branch, markers or repo-variables")
            ),
        ]
    );
}

#[test]
fn doctor_names_what_the_config_leaves_to_chance() {
    assert_eq!(
        doctored(&setup(CONFIG), &["doctor", "--layer", "actions"]),
        (DONE, format!("{READS}{DERIVED}{LISTED}"))
    );
    let keyed = setup(&format!(
        "{CONFIG}[markers]\nkey = {{ secret = \"HERALD_MARKER_SECRET\" }}\n"
    ));
    assert_eq!(
        doctored(&keyed, &["doctor"]),
        (
            DONE,
            format!(
                "{READS}{LISTED}Nothing that reads HERALD_MARKER_SECRET is checked, as it is not set here; herald doctor --online reads every secret.\n"
            )
        )
    );
    let d1 = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"d1\""));
    assert_eq!(
        doctored(&d1, &["doctor"]),
        (
            STOPPED,
            format!(
                "{READS}{DERIVED}store.kind: keeping records in d1 cannot be served: this herald keeps records in sqlite, postgres, dynamodb, git-branch, markers, actions-cache or repo-variables; set [store] kind to one of them. The serverless layer is the first that serves it.
"
            )
        )
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
        doctored(&setup(&config), &["doctor"])
    };
    assert_eq!(
        pinned(""),
        (
            STOPPED,
            format!(
                "{READS}{DERIVED}{REFS}routes[0].pin: a pin the store remembers cannot be served: the actions-cache store keeps no pins; add \"pins\" to its keeps, or set pin = \"markers\".
"
            )
        )
    );
    assert_eq!(
        pinned("keeps = [\"pins\"]"),
        (DONE, format!("{READS}{DERIVED}{REFS}"))
    );
}

#[test]
fn doctor_names_what_the_markers_store_cannot_serve() {
    let gaps = |config: &str| {
        let doctored = herald(&setup(config), &World::default(), &["doctor"]);
        assert_eq!(doctored.code, STOPPED, "{}", doctored.err);
        assert_eq!(
            doctored
                .out
                .lines()
                .filter(|line| line.starts_with("The markers store has no compare-and-set"))
                .count(),
            1
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
    assert_eq!(
        gaps(&forum),
        [
            "routes[0].destination: sending each announcement once cannot be served: the markers store reads announcements back from their posts, and a forum's posts are not read back; post through a bot to a text or announcement channel, or choose a store that keeps records."
        ]
    );
}

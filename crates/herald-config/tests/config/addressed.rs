//! Which routes post where anyone may read them, decided by each kind of
//! destination: such a route never says `private = true`, whatever
//! `[doctor] private_to_public` says, and is never shown an advisories
//! section (ADR-0067).

use herald_config::{Config, load};
use herald_core::Surface;
use herald_kinds::Checking;

/// Every kind of destination herald ships, whether a route to it as set
/// here posts where anyone may read it, and whether it edits a post again
/// from what herald kept of the route.
const DECIDED: [(&str, bool, bool); 7] = [
    ("discord", false, true),
    ("mastodon", true, true),
    ("matrix", false, true),
    ("slack", false, true),
    ("activitypub", true, true),
    ("herald", true, false),
    ("herald-push", true, false),
];

/// Every kind of destination herald ships, and how herald learns who may
/// read a route's place as set here before a run's first advisory send:
/// by the requests named, or never, as the kind is never sent a section.
const CHECKED: [(&str, Option<usize>); 7] = [
    ("discord", Some(3)),
    ("mastodon", None),
    ("matrix", Some(7)),
    ("slack", Some(1)),
    ("activitypub", None),
    ("herald", None),
    ("herald-push", None),
];

const FEDERATION: &str =
    "[federation]\ndomain = \"herald.example\"\nkeys = [{ secret = \"HERALD_KEY_NEW\" }]\n";

/// The destination of `kind`, a route to it with `extra`, and what else it
/// needs to load.
fn written(kind: &str, extra: &str) -> String {
    let (store, destination, placed, templated) = match kind {
        "discord" => (
            "kind = \"sqlite\"",
            "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
            "channel = \"1566200000000000100\"",
            true,
        ),
        "mastodon" => (
            "kind = \"sqlite\"",
            "server = \"https://mastodon.example\"\ntoken = { secret = \"HERALD_MASTODON_TOKEN\" }",
            "",
            true,
        ),
        "matrix" => (
            "kind = \"sqlite\"",
            "homeserver = \"https://matrix.example\"\ntoken = { secret = \"HERALD_MATRIX_TOKEN\" }",
            "room = \"!releases:matrix.example\"",
            true,
        ),
        "slack" => (
            "kind = \"sqlite\"",
            "token = { secret = \"HERALD_SLACK_TOKEN\" }",
            "channel = \"C0123456789\"",
            true,
        ),
        "activitypub" => (
            "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
            "domain = \"herald.example\"\nkey = { secret = \"HERALD_ACTIVITYPUB_KEY\" }",
            "actor = \"gate\"",
            true,
        ),
        "herald" => ("kind = \"sqlite\"", "", "", false),
        _ => (
            "kind = \"sqlite\"",
            "peer = \"upstream\"\ninbox = \"releases\"",
            "",
            false,
        ),
    };
    let peer = format!(
        "[peers.upstream]\nurl = \"https://peer.example/h\"\nkeys = [\"{}\"]\n",
        herald_peers::multibase::ed25519_public(&[5; 32])
    );
    let template = if templated { "template = \"t\"\n" } else { "" };
    format!(
        "version = 1\n[store]\n{store}\n{FEDERATION}{peer}[destinations.place]\nkind = \"{kind}\"\n{destination}\n[sources.gate]\nkind = \"github\"\nrepository = \"o/r\"\n[[routes]]\nid = \"posted\"\nsources = [\"gate\"]\ndestination = \"place\"\n{template}{placed}\n{extra}\n[templates.t]\ntitle = \"{{{{ event.title }}}}\"\nbody = \"{{{{ event.body }}}}\"\n"
    )
}

pub(crate) fn loaded(kind: &str, extra: &str) -> Result<Config, String> {
    load(
        "herald.toml",
        &written(kind, extra),
        &|_| Err("no file".to_owned()),
        &crate::common::kinds(),
    )
    .map_err(|errors| errors.to_string())
}

#[test]
fn every_kind_of_destination_decides_whether_its_routes_post_where_anyone_may_read() {
    let listed: Vec<&str> = DECIDED.iter().map(|(kind, _, _)| *kind).collect();
    assert_eq!(crate::common::kinds().destination_kinds(), listed);
    for (kind, public, _) in DECIDED {
        let config = loaded(kind, "").unwrap();
        let route = &config.routes[0];
        let destination = &config.destinations[&route.route.destination];
        assert_eq!(
            destination
                .public(&route.options, &route.route.delivery)
                .is_some(),
            public,
            "{kind}"
        );
        let declared = loaded(kind, "private = true");
        assert_eq!(declared.is_err(), public, "{kind}: {declared:?}");
        if let Ok(declared) = declared {
            assert_eq!(
                declared.surface(&declared.routes[0].route.id),
                Surface::Route {
                    private: true,
                    recallable: false
                },
                "{kind}"
            );
        }
        for guard in ["warn", "allow"] {
            let widened = loaded(
                kind,
                &format!("private = true\n[doctor]\nprivate_to_public = \"{guard}\""),
            );
            assert_eq!(widened.is_err(), public, "{kind} {guard}");
        }
    }
}

#[test]
fn every_kind_of_destination_decides_how_herald_learns_who_may_read_a_route_s_place() {
    let listed: Vec<&str> = CHECKED.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(crate::common::kinds().destination_kinds(), listed);
    for (kind, requests) in CHECKED {
        let config = loaded(kind, "").unwrap();
        let route = &config.routes[0];
        let checking = config.destinations[&route.route.destination].checking(&route.options);
        match (checking, requests) {
            (Checking::Asked { asks, requests }, Some(wanted)) => {
                assert_eq!(requests, wanted, "{kind}");
                assert!(!asks.is_empty(), "{kind}");
            }
            (Checking::Unadvised, None) => {}
            (other, _) => panic!("{kind}: {other:?}"),
        }
    }
    let private = loaded("mastodon", "visibility = \"private\"").unwrap();
    let route = &private.routes[0];
    assert!(matches!(
        private.destinations[&route.route.destination].checking(&route.options),
        Checking::Asked { requests: 1, .. }
    ));
}

/// `kind`'s destination reached by a webhook, its route set with `extra`.
fn hooked(kind: &str, extra: &str) -> Result<Config, String> {
    let (token, place) = match kind {
        "discord" => ("HERALD_DISCORD_TOKEN", "channel = \"1566200000000000100\""),
        _ => ("HERALD_SLACK_TOKEN", "channel = \"C0123456789\""),
    };
    let text = written(kind, extra)
        .replace(
            &format!("token = {{ secret = \"{token}\" }}"),
            "webhook = { secret = \"HERALD_WEBHOOK\" }",
        )
        .replace(place, "");
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::common::kinds(),
    )
    .map_err(|errors| errors.to_string())
}

#[test]
fn only_a_private_route_herald_cannot_check_may_be_vouched_for() {
    for kind in ["discord", "slack"] {
        let vouched = hooked(kind, "private = true\nvouch_unchecked = true").unwrap();
        let route = &vouched.routes[0];
        assert!(route.vouched, "{kind}");
        assert!(matches!(
            vouched.destinations[&route.route.destination].checking(&route.options),
            Checking::Unchecked(why) if why.contains("webhook")
        ));
        assert!(!hooked(kind, "private = true").unwrap().routes[0].vouched);
        let unprivate = hooked(kind, "vouch_unchecked = true").unwrap_err();
        assert!(
            unprivate.contains("routes[0].vouch_unchecked: vouch_unchecked vouches for a route that says private = true"),
            "{unprivate}"
        );
    }
    for (kind, refused) in [
        ("discord", "so there is nothing to vouch for"),
        ("matrix", "so there is nothing to vouch for"),
        ("herald", "is never sent an advisories section"),
    ] {
        let error = loaded(kind, "private = true\nvouch_unchecked = true").unwrap_err();
        assert!(error.contains(refused), "{kind}: {error}");
    }
    let public = loaded("mastodon", "vouch_unchecked = true\nprivate = true").unwrap_err();
    assert!(
        public.contains("routes[0].private: this route is declared private"),
        "{public}"
    );
}

#[test]
fn a_mastodon_route_posts_privately_only_to_its_followers_and_a_crosspost_publicly() {
    for (visibility, public) in [("private", false), ("unlisted", true), ("public", true)] {
        let set = format!("visibility = \"{visibility}\"\nprivate = true");
        let config = loaded("mastodon", &set);
        assert_eq!(config.is_err(), public, "{visibility}: {config:?}");
    }
    let refused = loaded("discord", "crosspost = true\nprivate = true").unwrap_err();
    assert!(
        refused.contains("routes[0].private: this route is declared private, but it crossposts each post to every server following the channel; remove private = true"),
        "{refused}"
    );
    let crossposted = loaded("discord", "crosspost = true").unwrap();
    assert_eq!(
        crossposted.surface(&crossposted.routes[0].route.id),
        Surface::Route {
            private: false,
            recallable: false
        }
    );
}

#[test]
fn a_private_grouped_route_is_a_digest_and_a_route_not_in_the_config_is_not_private() {
    let grouped = loaded(
        "discord",
        "private = true\ngroup = { key = \"{{ event.attributes.version }}\" }",
    )
    .unwrap();
    assert_eq!(
        grouped.surface(&grouped.routes[0].route.id),
        Surface::Digest
    );
    assert_eq!(
        grouped.surface(&herald_core::RouteId::new("gone")),
        Surface::Route {
            private: false,
            recallable: false
        }
    );
}

#[test]
fn a_route_s_place_is_written_without_a_secret_and_read_back_by_its_kind() {
    for (kind, _, recalled) in DECIDED {
        let config = loaded(kind, "").unwrap();
        let route = &config.routes[0];
        let destination = &config.destinations[&route.route.destination];
        let written = route.options.written();
        assert_eq!(written.is_some(), recalled, "{kind}");
        let Some(written) = written else {
            continue;
        };
        assert!(!written.contains("HERALD_"), "{kind}: {written}");
        assert!(!written.contains("secret"), "{kind}: {written}");
        let placed = destination.recalled(&written).unwrap();
        assert_eq!(
            placed.written().as_deref(),
            Some(written.as_str()),
            "{kind}"
        );
        assert_eq!(placed.place(), route.options.place(), "{kind}");
    }
}

#[test]
fn herald_s_fixed_recall_line_cannot_be_replaced() {
    let replaced = loaded(
        "discord",
        "[templates.\"herald:recalled\"]\ntitle = \"{{ view }}\"\nbody = \"{{ view }}\"",
    )
    .unwrap_err();
    assert!(
        replaced.contains("herald:recalled is herald's own fixed line and cannot be replaced"),
        "{replaced}"
    );
}

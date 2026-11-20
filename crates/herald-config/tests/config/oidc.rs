//! `[apps.*] oidc` and `peers`: the ID tokens an app takes as credentials,
//! each entry read exactly and refused where it would take a token of
//! any run, and the names an app stands for when its runs push into a
//! herald source.

use std::collections::{BTreeMap, BTreeSet};

use herald_config::{Config, Oidc, Replay, Taking, load};
use herald_core::{ClaimPattern, TokenAlgorithm};
use herald_peers::multibase::ed25519_public;
use jiff::SignedDuration;

const ISSUER: &str = "https://token.actions.githubusercontent.com";

const FEDERATION: &str = r#"
[federation]
domain = "herald.example"
keys = [{ secret = "HERALD_KEY_NEW" }]
"#;

fn text(store: &str, rest: &str) -> String {
    format!(
        "version = 1\n[store]\nkind = \"{store}\"\n{}\
         [destinations.discord]\nkind = \"discord\"\ntoken = {{ secret = \"T\" }}\n\
         [templates.t]\ntitle = \"{{{{ event.title }}}}\"\nbody = \"{{{{ event.body }}}}\"\n{rest}",
        if store == "markers" {
            ""
        } else {
            "key = { secret = \"HERALD_STORE_KEY\" }\n"
        }
    )
}

/// A route taking the events of the app `app`.
fn route(app: &str) -> String {
    format!(
        "[[routes]]\nid = \"{app}\"\nsources = [\"{app}\"]\ndestination = \"discord\"\n\
         channel = \"1566200000000000100\"\ntemplate = \"t\"\nprivate = false\n"
    )
}

fn loaded(store: &str, rest: &str) -> Result<Config, String> {
    load(
        "herald.toml",
        &text(store, rest),
        &|_| Err(String::new()),
        &crate::common::kinds(),
    )
    .map_err(|errors| errors.to_string())
}

fn app(entry: &str) -> String {
    format!(
        "{}[apps.ci]\nprojects = [\"herald\"]\noidc = [{{ {entry} }}]\n",
        route("ci")
    )
}

fn bound(entry: &str) -> String {
    app(&format!(
        "issuer = \"{ISSUER}\", claims = {{ repository_id = \"42\" }}{entry}"
    ))
}

fn refused(store: &str, rest: &str, said: &str) {
    let errors = loaded(store, rest).unwrap_err();
    assert!(errors.contains(said), "{said} is not in {errors}");
}

#[test]
fn an_entry_reads_its_issuer_exactly_its_claims_and_its_defaults() {
    let config = loaded(
        "git-branch",
        &format!(
            "[server]\nurl = \"https://herald.example/h\"\n{}",
            bound("")
        ),
    )
    .unwrap();
    assert_eq!(
        config.apps["ci"].oidc,
        [Oidc {
            issuer: ISSUER.to_owned(),
            audience: "https://herald.example/h".to_owned(),
            claims: BTreeMap::from([("repository_id".to_owned(), ClaimPattern::new("42"))]),
            algorithms: vec![TokenAlgorithm::Rs256],
            keys_for: Oidc::KEYS_FOR,
            keys_retry: Oidc::KEYS_RETRY,
            jwks: None,
            keys: None,
            takes: BTreeSet::from([Taking::Announce]),
            lifetime: Oidc::LIFETIME,
            skew: Oidc::SKEW,
        }]
    );
    assert!(config.apps["ci"].peers.is_empty());
    assert_eq!(config.apps["ci"].replay, Replay::Once);
    let domain = loaded("git-branch", &format!("{FEDERATION}{}", bound(""))).unwrap();
    assert_eq!(domain.apps["ci"].oidc[0].audience, "https://herald.example");
    let written = loaded(
        "git-branch",
        &bound(
            ", audience = \"https://herald.example/ci\", algorithms = [\"RS256\"], keys_for = \"1d\", keys_retry = \"5m\", jwks = \"https://keys.example/jwks\", lifetime = \"1d\", skew = \"5m\"",
        ),
    )
    .unwrap();
    let entry = &written.apps["ci"].oidc[0];
    assert_eq!(entry.audience, "https://herald.example/ci");
    assert_eq!(entry.keys_for, SignedDuration::from_hours(24));
    assert_eq!(entry.keys_retry, SignedDuration::from_mins(5));
    assert_eq!(entry.jwks.as_deref(), Some("https://keys.example/jwks"));
    assert_eq!(entry.bases(), [ISSUER, "https://keys.example/jwks"]);
    assert_eq!(
        (entry.lifetime, entry.skew),
        (SignedDuration::from_hours(24), SignedDuration::from_mins(5))
    );
    assert!(entry.binds_an_id());
}

#[test]
fn pinned_keys_are_read_as_written_and_need_no_store_that_keeps_them() {
    let pinned = bound(
        ", audience = \"https://herald.example\", keys = [{ kty = \"RSA\", kid = \"k1\", n = \"AQID\", e = \"AQAB\" }]",
    );
    let config = loaded("markers", &pinned).unwrap();
    let entry = &config.apps["ci"].oidc[0];
    let keys = entry.keys.as_ref().unwrap();
    assert_eq!(keys.key(Some("k1")).unwrap().modulus, [1, 2, 3]);
    assert_eq!(entry.bases(), Vec::<String>::new());
    refused(
        "markers",
        &bound(", audience = \"https://herald.example\""),
        "the markers store keeps no records, so it keeps no issuer's keys between requests",
    );
    refused(
        "git-branch",
        &bound(
            ", audience = \"https://herald.example\", keys = [{ kty = \"EC\", crv = \"P-256\" }]",
        ),
        "no key pinned is an RSA signing key",
    );
    refused(
        "git-branch",
        &bound(
            ", audience = \"https://herald.example\", jwks = \"https://keys.example\", keys = [{ kty = \"RSA\", n = \"AQID\", e = \"AQAB\" }]",
        ),
        "pinned keys are never read; leave jwks out, or the keys",
    );
}

#[test]
fn an_entry_that_binds_no_one_run_is_refused() {
    let any = "the entry binds no claim naming who a run is to one id or one owner, and this would accept a token of any repository at this issuer";
    let audience = "audience = \"https://herald.example\"";
    let entry = |claims: &str| {
        app(&format!(
            "issuer = \"{ISSUER}\", {audience}, claims = {{ {claims} }}"
        ))
    };
    refused(
        "git-branch",
        &app(&format!("issuer = \"{ISSUER}\", {audience}")),
        any,
    );
    for loose in [
        "ref = \"refs/heads/main\"",
        "event_name = \"push\"",
        "aud = \"https://herald.example\"",
        &format!("iss = \"{ISSUER}\""),
        "repository_visibility = \"public\"",
        "workflow_ref = \"org/plugin/.github/workflows/ci.yml@refs/heads/main\"",
        "repository_owner = \"org*\"",
        "repository_id = \"*\"",
        "repository_id = \"4?\"",
        "project_id = \"7*\"",
        "repository = \"*/plugin\"",
        "repository = \"org?/plugin\"",
        "repository = \"/plugin\"",
        "sub = \"repo:*\"",
        "sub = \"x\"",
        "sub = \"ref:refs/heads/main\"",
        "sub = \"environment:prod\"",
        "sub = \"job_workflow_ref:org/shared/.github/workflows/release.yml@refs/heads/main\"",
        "sub = \"context:org/app\"",
        "sub = \"repo:org\"",
        "sub = \"repo:/app\"",
        "sub = \"repo:org*/app\"",
        "sub = \"project_path:*/app\"",
        "sub = \"re?o:org/app\"",
        "namespace_path = \"*\"",
        "repository = \"*\", ref = \"refs/heads/main\"",
    ] {
        refused("git-branch", &entry(loose), any);
    }
    refused(
        "git-branch",
        &entry("repository = \"\""),
        "a claim is named, and its pattern says what it must be",
    );
    for (claims, id) in [
        ("repository_id = \"42\"", true),
        ("repository_owner_id = \"7\", ref = \"refs/heads/*\"", true),
        ("project_id = \"7\"", true),
        ("namespace_id = \"7\"", true),
        ("repository = \"org/*\"", false),
        ("repository = \"org/plugin\"", false),
        ("repository_owner = \"org\"", false),
        ("sub = \"repo:org/*:ref:refs/heads/main\"", false),
        ("sub = \"repo:acme/app:ref:refs/heads/main\"", false),
        (
            "sub = \"project_path:group/app:ref_type:branch:ref:main\"",
            false,
        ),
        ("project_path = \"group/*\"", false),
        ("namespace_path = \"group\"", false),
        ("repository_id = \"*\", repository = \"org/*\"", false),
    ] {
        let named = loaded("git-branch", &entry(claims)).unwrap();
        let entry = &named.apps["ci"].oidc[0];
        assert_eq!(entry.binds_an_id(), id, "{claims}");
    }
}

#[test]
fn each_key_of_an_entry_is_held_to_what_herald_reads() {
    let audience = ", audience = \"https://herald.example\"";
    let cases: [(String, &str); 15] = [
        (
            bound(&format!("{audience}, keys_retry = \"9s\"")),
            "keys_retry is from \"10s\" to keys_for; give it within them",
        ),
        (
            bound(&format!(
                "{audience}, keys_for = \"1m\", keys_retry = \"61s\""
            )),
            "keys_retry is from \"10s\" to keys_for; give it within them",
        ),
        (
            app("issuer = \"http://issuer.example\", claims = { repository_id = \"1\" }"),
            "\"http://issuer.example\" is not an issuer herald reads",
        ),
        (
            app("issuer = \"https://issuer.example/?q\", claims = { repository_id = \"1\" }"),
            "is not an issuer herald reads",
        ),
        (bound(", audience = \"\""), "the audience is empty"),
        (
            bound(&format!(", audience = \"{ISSUER}\"")),
            "the audience is the issuer",
        ),
        (
            bound(", audience = \"https://github.com/nightworksio\""),
            "the audience is GitHub's default",
        ),
        (
            bound(", audience = \"https://github.com/nightworksio/\""),
            "the audience is GitHub's default",
        ),
        (
            bound(&format!("{audience}, lifetime = \"25h\"")),
            "lifetime is at most \"1d\"; shorten it",
        ),
        (
            bound(&format!("{audience}, skew = \"6m\"")),
            "skew is at most \"5m\"; shorten it",
        ),
        (
            bound(&format!("{audience}, algorithms = []")),
            "no algorithm is allowed, so no token is taken",
        ),
        (
            bound(&format!("{audience}, algorithms = [\"HS256\"]")),
            "algorithms",
        ),
        (
            bound(&format!("{audience}, keys_for = \"2d\"")),
            "keys_for is at most \"1d\"; shorten it",
        ),
        (
            bound(&format!("{audience}, jwks = \"http://keys.example\"")),
            "\"http://keys.example\" is not where herald reads keys",
        ),
        (
            bound(""),
            "a token must name this herald as its audience, and herald knows no URL of its own",
        ),
    ];
    for (rest, said) in cases {
        refused("git-branch", &rest, said);
    }
    for within in [
        ", keys_retry = \"10s\"",
        ", keys_for = \"1m\", keys_retry = \"1m\"",
    ] {
        let config = loaded("git-branch", &bound(&format!("{audience}{within}")));
        assert_eq!(config.err(), None, "{within}");
    }
    let github_owned = loaded(
        "git-branch",
        &bound(", audience = \"https://github.com/nightworksio/herald\""),
    );
    assert!(github_owned.is_ok());
    let widest = loaded(
        "git-branch",
        &bound(&format!("{audience}, skew = \"5m\", lifetime = \"24h\"")),
    );
    assert_eq!(widest.err(), None);
}

#[test]
fn an_enterprise_github_s_default_audience_is_refused_as_github_com_s_is() {
    let at = |issuer: &str, audience: &str| {
        app(&format!(
            "issuer = \"{issuer}\", audience = \"{audience}\", claims = {{ repository_id = \"42\" }}"
        ))
    };
    let default = "the audience is GitHub's default";
    for (issuer, audience) in [
        (
            "https://ghes.example/_services/token",
            "https://ghes.example/org",
        ),
        (
            "https://ghes.example/_services/token/",
            "https://ghes.example/org/",
        ),
        (
            "https://ghes.example:8443/_services/token",
            "https://ghes.example:8443/org",
        ),
        (
            "https://token.actions.octo.ghe.com",
            "https://octo.ghe.com/org",
        ),
        (
            "https://token.actions.githubusercontent.com/octo-enterprise",
            "https://github.com/org",
        ),
        (
            "https://ghes.example/_services/token",
            "https://github.com/org",
        ),
    ] {
        refused("git-branch", &at(issuer, audience), default);
    }
    for (issuer, audience) in [
        (
            "https://ghes.example/_services/token",
            "https://ghes.example/org/herald",
        ),
        (
            "https://ghes.example/_services/token",
            "https://herald.ghes.example",
        ),
        (
            "https://ghes.example/_services/token",
            "https://other.example/org",
        ),
        (
            "https://token.actions.octo.ghe.com",
            "https://other.ghe.com/org",
        ),
        ("https://ghes.example/other", "https://ghes.example/org"),
        ("https://gitlab.example", "https://gitlab.example/org"),
    ] {
        assert_eq!(
            loaded("git-branch", &at(issuer, audience)).err(),
            None,
            "{issuer} {audience}"
        );
    }
}

#[test]
fn every_entry_naming_an_issuer_reads_its_keys_alike() {
    let other = format!(
        "{}[apps.deploy]\nprojects = [\"herald\"]\noidc = [{{ issuer = \"{ISSUER}\", audience = \"https://herald.example\", claims = {{ repository_id = \"7\" }}, keys_for = \"2h\" }}]\n",
        route("deploy")
    );
    refused(
        "git-branch",
        &format!(
            "{}{other}",
            bound(", audience = \"https://herald.example\"")
        ),
        "apps.ci reads the keys of https://token.actions.githubusercontent.com otherwise",
    );
    refused(
        "git-branch",
        &format!(
            "{}{}",
            bound(", audience = \"https://herald.example\""),
            other.replace("keys_for = \"2h\"", "keys_retry = \"2m\"")
        ),
        "give every entry naming an issuer the same jwks, keys, keys_for and keys_retry",
    );
    let alike = other.replace(", keys_for = \"2h\"", "");
    assert!(
        loaded(
            "git-branch",
            &format!(
                "{}{alike}",
                bound(", audience = \"https://herald.example\"")
            )
        )
        .is_ok()
    );
}

fn standing(peers: &str, grants: &str) -> String {
    format!(
        "{FEDERATION}[apps.runs]\noidc = [{{ issuer = \"{ISSUER}\", claims = {{ repository_id = \"42\" }} }}]\npeers = [{peers}]\n\
         [sources.inbox]\nkind = \"herald\"\npeers = {{ {grants} }}\n"
    )
}

#[test]
fn an_app_stands_for_names_a_herald_source_alone_grants() {
    let config = loaded(
        "git-branch",
        &standing(
            "\"plugin-ci\"",
            "plugin-ci = { actions = [\"push\"], namespaces = [\"github.com/42\"] }",
        ),
    )
    .unwrap();
    let app = &config.apps["runs"];
    assert!(app.peers.contains("plugin-ci"));
    assert!(app.projects.is_empty() && app.routes.is_empty());
    let source = &config.sources[&herald_core::SourceName::new("inbox")];
    assert!(source.grants().unwrap().contains_key("plugin-ci"));
}

#[test]
fn a_name_an_app_stands_for_is_its_own_and_granted_one_to_a_source() {
    let push = "actions = [\"push\"]";
    refused(
        "git-branch",
        &standing("\"plugin-ci\"", &format!("other = {{ {push} }}")),
        "there is no peer \"other\" in [peers], and no app stands for it",
    );
    refused(
        "git-branch",
        &standing(
            "\"plugin-ci\", \"docs-ci\"",
            &format!("plugin-ci = {{ {push} }}, docs-ci = {{ {push} }}"),
        ),
        "apps.runs stands for \"docs-ci\" too, which sources.inbox grants as well",
    );
    refused(
        "git-branch",
        &standing("\"Plugin CI\"", ""),
        "\"Plugin CI\" is not a peer's name",
    );
    let peer = format!(
        "[peers.plugin-ci]\nurl = \"https://peer.example\"\nkeys = [\"{}\"]\n",
        ed25519_public(&[5; 32])
    );
    refused(
        "git-branch",
        &format!("{}{peer}", standing("\"plugin-ci\"", "")),
        "\"plugin-ci\" is a table of [peers]",
    );
    let again = format!(
        "[apps.again]\noidc = [{{ issuer = \"{ISSUER}\", claims = {{ repository_id = \"7\" }} }}]\npeers = [\"plugin-ci\"]\n"
    );
    refused(
        "git-branch",
        &format!("{}{again}", standing("\"plugin-ci\"", "")),
        "stands for \"plugin-ci\" too; give each name one app",
    );
    refused(
        "git-branch",
        &format!("{FEDERATION}[apps.tokens]\npeers = [\"plugin-ci\"]\n"),
        "an app stands for peers only where a run proves who it is with an ID token",
    );
    refused(
        "git-branch",
        &standing("\"plugin-ci\"", "")
            .replace(FEDERATION, "[server]\nurl = \"https://herald.example\"\n"),
        "what an app pushes as a peer is proved by [federation]'s newest key on its domain",
    );
    refused(
        "git-branch",
        &standing("\"plugin-ci\"", "").replace("keys = [{ secret = \"HERALD_KEY_NEW\" }]\n", ""),
        "what an app pushes as a peer is proved by [federation]'s newest key on its domain",
    );
    refused(
        "git-branch",
        &standing("\"plugin-ci\"", "").replace(
            "peers = [\"plugin-ci\"]\n",
            "peers = [\"plugin-ci\"]\nroutes = [\"runs\"]\n",
        ),
        "an app that announces no project announces to no route",
    );
    refused(
        "git-branch",
        &format!(
            "{FEDERATION}[apps.nothing]\noidc = [{{ issuer = \"{ISSUER}\", claims = {{ repository_id = \"7\" }} }}]\n"
        ),
        "an app announces only the projects it names",
    );
}

#[test]
fn a_push_presenting_the_run_s_id_token_needs_no_federation_key() {
    let config = loaded(
        "git-branch",
        &format!(
            "[peers.org]\nurl = \"https://org.example\"\nkeys = [\"{}\"]\n\
             [destinations.org]\nkind = \"herald-push\"\npeer = \"org\"\ninbox = \"repos\"\ncredential = \"id-token\"\n\
             [sources.gate]\nkind = \"github\"\nrepository = \"org/plugin\"\n\
             [[routes]]\nid = \"pushed\"\nsources = [\"gate\"]\ndestination = \"org\"\nprivate = false\n",
            ed25519_public(&[5; 32]),
        ),
    )
    .unwrap();
    let destination = &config.destinations[&herald_core::DestinationName::new("org")];
    assert_eq!(
        config
            .credential(destination)
            .map(|named| named.secret.as_str()),
        Some("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
    );
    for uses in [
        herald_config::Uses::Announces,
        herald_config::Uses::Polls,
        herald_config::Uses::Serves,
    ] {
        let used = config.secrets_used(uses);
        assert!(used.contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
        assert!(used.contains("ACTIONS_ID_TOKEN_REQUEST_URL"));
    }
}

#[test]
fn an_entry_takes_what_its_app_does_and_says_so_where_the_app_does_both() {
    let pushing = loaded(
        "git-branch",
        &standing("\"plugin-ci\"", "plugin-ci = { actions = [\"push\"] }"),
    )
    .unwrap();
    assert_eq!(
        pushing.apps["runs"].oidc[0].takes,
        BTreeSet::from([Taking::Push])
    );
    let both = |takes: &str| {
        format!(
            "{FEDERATION}{}[apps.ci]\nprojects = [\"herald\"]\npeers = [\"plugin-ci\"]\n\
             oidc = [{{ issuer = \"{ISSUER}\", claims = {{ repository_id = \"42\" }}{takes} }}]\n\
             [sources.inbox]\nkind = \"herald\"\npeers = {{ plugin-ci = {{ actions = [\"push\"] }} }}\n",
            route("ci")
        )
    };
    let config = loaded("git-branch", &both(", takes = [\"push\", \"announce\"]")).unwrap();
    assert_eq!(
        config.apps["ci"].oidc[0].takes,
        BTreeSet::from([Taking::Announce, Taking::Push])
    );
    let pushed = loaded("git-branch", &both(", takes = [\"push\"]")).unwrap();
    assert_eq!(
        pushed.apps["ci"].oidc[0].takes,
        BTreeSet::from([Taking::Push])
    );
    refused(
        "git-branch",
        &both(""),
        "apps.ci.oidc[0].takes: the app announces and stands for peers; say what this entry's tokens may do",
    );
    refused(
        "git-branch",
        &both(", takes = []"),
        "the entry's tokens may do nothing, so none is taken",
    );
    refused(
        "git-branch",
        &bound(", audience = \"https://herald.example\", takes = [\"push\"]"),
        "the entry's tokens may do what the app does not",
    );
}

#[test]
fn an_audience_another_herald_is_known_by_is_refused() {
    let peer = format!(
        "[peers.org]\nurl = \"https://org.example/\"\nkeys = [\"{}\"]\n",
        ed25519_public(&[5; 32])
    );
    refused(
        "git-branch",
        &format!("{peer}{}", bound(", audience = \"https://org.example\"")),
        "apps.ci.oidc[0].audience: the audience is the URL of peers.org, another herald",
    );
    assert!(
        loaded(
            "git-branch",
            &format!("{peer}{}", bound(", audience = \"https://herald.example\""))
        )
        .is_ok()
    );
}

#[test]
fn an_app_takes_each_token_once_where_its_store_keeps_deliveries() {
    let pinned = ", audience = \"https://herald.example\", keys = [{ kty = \"RSA\", kid = \"k1\", n = \"AQID\", e = \"AQAB\" }]";
    let kept = loaded("git-branch", &bound(pinned)).unwrap();
    assert_eq!(kept.apps["ci"].replay, Replay::Once);
    let unkept = loaded("markers", &bound(pinned)).unwrap();
    assert_eq!(unkept.apps["ci"].replay, Replay::UntilExpiry);
    let chosen = |replay: &str| {
        bound(pinned).replace(
            "[apps.ci]\n",
            &format!("[apps.ci]\nreplay = \"{replay}\"\n"),
        )
    };
    assert_eq!(
        loaded("git-branch", &chosen("until-expiry")).unwrap().apps["ci"].replay,
        Replay::UntilExpiry
    );
    assert_eq!(
        loaded("markers", &chosen("until-expiry")).unwrap().apps["ci"].replay,
        Replay::UntilExpiry
    );
    assert_eq!(
        loaded("git-branch", &chosen("once")).unwrap().apps["ci"].replay,
        Replay::Once
    );
    refused(
        "markers",
        &chosen("once"),
        "apps.ci.replay: the markers store keeps no deliveries, so it cannot remember a token taken; choose a store that keeps records, or replay = \"until-expiry\"",
    );
    refused(
        "git-branch",
        &format!(
            "{}[apps.tokens]\nprojects = [\"herald\"]\nreplay = \"once\"\n",
            route("tokens")
        ),
        "apps.tokens.replay: replay says how often an ID token is taken, and the app takes none",
    );
    refused("git-branch", &chosen("twice"), "replay");
}

#[test]
fn a_push_s_audience_is_on_its_peer_s_origin() {
    let push = |audience: &str| {
        format!(
            "[peers.org]\nurl = \"https://org.example/h\"\nkeys = [\"{}\"]\n\
             [destinations.org]\nkind = \"herald-push\"\npeer = \"org\"\ninbox = \"repos\"\ncredential = \"id-token\"\n{audience}\
             [sources.gate]\nkind = \"github\"\nrepository = \"org/plugin\"\n\
             [[routes]]\nid = \"pushed\"\nsources = [\"gate\"]\ndestination = \"org\"\nprivate = false\n",
            ed25519_public(&[5; 32]),
        )
    };
    for aimed in [
        "",
        "audience = \"https://org.example/h\"\n",
        "audience = \"https://org.example/ci\"\n",
    ] {
        assert_eq!(loaded("git-branch", &push(aimed)).err(), None, "{aimed}");
    }
    for elsewhere in [
        "https://other.example",
        "https://org.example:8443/h",
        "http://org.example/h",
        "https://user@org.example/h",
        "https://evil.org.example",
        "not a url",
    ] {
        refused(
            "git-branch",
            &push(&format!("audience = \"{elsewhere}\"\n")),
            &format!(
                "destinations.org.audience: \"{elsewhere}\" is not on the origin of peers.org, https://org.example/h"
            ),
        );
    }
}

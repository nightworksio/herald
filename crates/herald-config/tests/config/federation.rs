//! `[federation]`: the instance's domain and keys, and the keys an
//! ActivityPub destination takes from it where its own table leaves them
//! out.

use herald_config::{Config, Federation, Uses, load};
use herald_core::SecretRef;
use herald_dest_fediverse::{ActorOptions, Approve, FollowerCaps, Mode};
use jiff::SignedDuration;

const ROUTED: &str = r#"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "followed"
sources = ["gate"]
destination = "fedi"
template = "t"
actor = "php-mutation-gate"

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn loaded(head: &str) -> Result<Config, String> {
    let text = format!(
        "version = 1\n\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"HERALD_STORE_KEY\" }}\n{head}{ROUTED}"
    );
    load("herald.toml", &text, &nothing, &crate::common::kinds())
        .map_err(|errors| errors.to_string())
}

fn actor(config: &Config) -> ActorOptions {
    config
        .destinations
        .values()
        .find_map(|destination| destination.of::<ActorOptions>())
        .cloned()
        .unwrap()
}

fn secret(name: &str) -> SecretRef {
    SecretRef {
        secret: name.to_owned(),
    }
}

const FEDERATION: &str = r#"
[federation]
domain = "herald.example"
keys = [{ secret = "HERALD_KEY_NEW" }, { secret = "HERALD_KEY_OLD" }]
actor_key = { secret = "HERALD_ACTOR_KEY" }
mode = "allowlist"
allow = ["friends.example"]
block = ["spam.example"]
followers = { most = 10 }
approve = "manual"
"#;

#[test]
fn the_instance_s_domain_and_keys_are_read_newest_first() {
    let config = loaded(&format!(
        "{FEDERATION}\n[destinations.fedi]\nkind = \"activitypub\"\n"
    ))
    .unwrap();
    assert_eq!(
        config.federation,
        Federation {
            domain: Some("herald.example".to_owned()),
            keys: vec![secret("HERALD_KEY_NEW"), secret("HERALD_KEY_OLD")],
            refresh: Federation::REFRESH,
            signed_for: Federation::SIGNED_FOR,
            fields: Federation::FIELDS,
            revoked: Vec::new(),
            keep_audit: Federation::KEEP_AUDIT,
            grants_ending: Federation::GRANTS_ENDING,
            stranger_rate: herald_config::Peer::RATE,
            replay_rate: herald_config::Peer::RATE,
            refused_nonces: Federation::REFUSED_NONCES,
            pages: None,
        }
    );
    assert_eq!(Federation::FIELDS, 8192);
    assert_eq!(Federation::FIELDS_ALLOWED, 32768);
    assert_eq!(Federation::REFRESH, SignedDuration::from_hours(24));
    assert_eq!(Federation::SIGNED_FOR, SignedDuration::from_mins(5));
    assert_eq!(
        Federation::SIGNED_FOR_ALLOWED,
        SignedDuration::from_mins(20)
    );
}

#[test]
fn the_instance_s_keys_are_read_only_by_a_run_that_serves() {
    let config = loaded(&format!(
        "{FEDERATION}\n[destinations.fedi]\nkind = \"activitypub\"\n"
    ))
    .unwrap();
    for key in ["HERALD_KEY_NEW", "HERALD_KEY_OLD"] {
        assert!(config.secrets_used(Uses::Serves).contains(key));
        assert!(!config.secrets_used(Uses::Polls).contains(key));
        assert!(!config.secrets_used(Uses::Announces).contains(key));
    }
}

#[test]
fn refresh_is_read_within_its_bounds() {
    let read = |set: &str| {
        loaded(&format!(
            "\n[federation]\n{set}\n\n[destinations.fedi]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = {{ secret = \"OWN\" }}\n"
        ))
    };
    let set = read("refresh = \"7d\"").unwrap().federation;
    assert_eq!(set.refresh, Federation::REFRESH_ALLOWED);
    assert_eq!(
        read("refresh = \"169h\"").err().as_deref(),
        Some("herald.toml:7:1 federation.refresh: refresh is at most \"7d\"; shorten it")
    );
}

#[test]
fn keep_audit_and_grants_ending_are_read_within_their_bounds() {
    let read = |set: &str| {
        loaded(&format!(
            "\n[federation]\n{set}\n\n[destinations.fedi]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = {{ secret = \"OWN\" }}\n"
        ))
    };
    let set = read("keep_audit = \"365d\"\ngrants_ending = \"90d\"")
        .unwrap()
        .federation;
    assert_eq!(
        (set.keep_audit, set.grants_ending),
        (
            Federation::KEEP_AUDIT_ALLOWED,
            Federation::GRANTS_ENDING_ALLOWED
        )
    );
    assert_eq!(Federation::KEEP_AUDIT, SignedDuration::from_hours(90 * 24));
    assert_eq!(
        Federation::GRANTS_ENDING,
        SignedDuration::from_hours(7 * 24)
    );
    assert_eq!(
        read("keep_audit = \"366d\"").err().as_deref(),
        Some("herald.toml:7:1 federation.keep_audit: keep_audit is at most \"365d\"; shorten it")
    );
    assert_eq!(
        read("stranger_rate = \"480/m\"")
            .unwrap()
            .federation
            .stranger_rate,
        herald_config::Peer::RATE_ALLOWED
    );
    assert_eq!(
        read("replay_rate = \"481/m\"").err().as_deref(),
        Some("herald.toml:7:1 federation.replay_rate: replay_rate is at most \"480/m\"; lower it")
    );
    let lives = |rate: herald_core::Rate| {
        let spans = Federation::SIGNED_FOR_ALLOWED.as_secs() / rate.per.length().as_secs();
        u32::try_from(spans).unwrap() * rate.count.get()
    };
    assert_eq!(
        (
            Federation::REFUSED_NONCES,
            Federation::REFUSED_NONCES_ALLOWED
        ),
        (
            lives(herald_config::Peer::RATE),
            lives(herald_config::Peer::RATE_ALLOWED)
        )
    );
    assert_eq!(
        read("refused_nonces = 9600")
            .unwrap()
            .federation
            .refused_nonces,
        9600
    );
    assert_eq!(
        read("refused_nonces = 9601").err().as_deref(),
        Some("herald.toml:7:1 federation.refused_nonces: refused_nonces is at most 9600; lower it")
    );
    assert_eq!(
        read("stranger_rate = \"481/m\"").err().as_deref(),
        Some(
            "herald.toml:7:1 federation.stranger_rate: stranger_rate is at most \"480/m\"; lower it"
        )
    );
    assert_eq!(
        read("grants_ending = \"91d\"").err().as_deref(),
        Some(
            "herald.toml:7:1 federation.grants_ending: grants_ending is at most \"90d\"; shorten it"
        )
    );
}

#[test]
fn fields_is_read_within_its_bounds() {
    let read = |set: &str| {
        loaded(&format!(
            "\n[federation]\n{set}\n\n[destinations.fedi]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = {{ secret = \"OWN\" }}\n"
        ))
    };
    assert_eq!(read("fields = \"32KiB\"").unwrap().federation.fields, 32768);
    assert_eq!(read("fields = \"1KiB\"").unwrap().federation.fields, 1024);
    assert_eq!(
        read("fields = \"33KiB\"").err().as_deref(),
        Some("herald.toml:7:1 federation.fields: fields is at most \"32KiB\"; lower it")
    );
}

#[test]
fn an_activitypub_destination_takes_what_its_table_leaves_out_from_federation() {
    let config = loaded(&format!(
        "{FEDERATION}\n[destinations.fedi]\nkind = \"activitypub\"\n"
    ))
    .unwrap();
    let actor = actor(&config);
    assert_eq!(actor.domain, "herald.example");
    assert_eq!(actor.key, secret("HERALD_ACTOR_KEY"));
    assert_eq!(actor.mode, Mode::Allowlist);
    assert_eq!(actor.allow, ["friends.example"]);
    assert_eq!(actor.block, ["spam.example"]);
    assert_eq!(actor.approve, Approve::Manual);
    assert_eq!(
        actor.followers,
        Some(FollowerCaps {
            most: Some(10),
            per_domain: None
        })
    );
}

#[test]
fn what_an_activitypub_destination_names_itself_is_its_own() {
    let config = loaded(&format!(
            "{FEDERATION}\n[destinations.fedi]\nkind = \"activitypub\"\ndomain = \"actors.example\"\nkey = {{ secret = \"OWN\" }}\nmode = \"open\"\nblock = []\napprove = \"auto\"\n"
        ))
        .unwrap();
    let actor = actor(&config);
    assert_eq!(actor.domain, "actors.example");
    assert_eq!(actor.key, secret("OWN"));
    assert_eq!(actor.mode, Mode::Open);
    assert_eq!(actor.block, Vec::<String>::new());
    assert_eq!(actor.approve, Approve::Auto);
    assert_eq!(actor.allow, ["friends.example"]);
    assert_eq!(config.federation.domain.as_deref(), Some("herald.example"));
}

#[test]
fn a_config_without_federation_has_no_domain_or_keys() {
    let config = loaded(
            "\n[destinations.fedi]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = { secret = \"OWN\" }\n",
        )
        .unwrap();
    assert_eq!(config.federation, Federation::default());
}

#[test]
fn an_activitypub_destination_with_no_domain_or_key_anywhere_names_both() {
    assert_eq!(
            loaded("\n[destinations.fedi]\nkind = \"activitypub\"\n").err(),
            Some(
                "herald.toml:7:1 destinations.fedi: give its domain, or domain in [federation]\n\
                 herald.toml:7:1 destinations.fedi: give its key, or actor_key in [federation]\n\
                 herald.toml:17:15 routes[0].destination: no destination is named \"fedi\"; the destinations are "
                    .to_owned()
            )
        );
}

#[test]
fn a_federation_domain_that_is_no_host_or_a_policy_that_cannot_hold_is_refused() {
    for (federation, error) in [
        (
            "domain = \"https://herald.example\"",
            "herald.toml:7:1 federation.domain: \"https://herald.example\" is not a domain; give the host herald serves on, such as \"herald.example\", with no scheme, port or path",
        ),
        (
            "block = [\"spam\"]",
            "herald.toml:7:1 federation: \"spam\" is neither a domain nor an actor's id; give a host such as \"spam.example\", or an id such as \"https://spam.example/users/x\"",
        ),
        (
            "keys = [{ secret = \"HERALD_KEY\" }]",
            "herald.toml:7:1 federation.domain: give the domain the keys are published on: domain = \"herald.example\"",
        ),
        (
            "followers = { per_domain = 0 }",
            "herald.toml:7:1 federation: followers.most and followers.per_domain take at least 1; leave one out for no limit",
        ),
    ] {
        let errors = loaded(&format!(
                "\n[federation]\n{federation}\n\n[destinations.fedi]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = {{ secret = \"OWN\" }}\n"
            ))
            .err()
            .unwrap_or_default();
        assert!(errors.starts_with(error), "{errors}");
    }
}

#[test]
fn a_federation_that_is_no_table_is_refused_and_lends_nothing() {
    let text = format!(
        "version = 1\nfederation = 1\n\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"HERALD_STORE_KEY\" }}\n\n[destinations.fedi]\nkind = \"activitypub\"\n{ROUTED}"
    );
    let errors = load("herald.toml", &text, &nothing, &crate::common::kinds())
        .err()
        .map(|errors| errors.to_string())
        .unwrap_or_default();
    assert!(
        errors.starts_with("herald.toml:2:14 federation:"),
        "{errors}"
    );
    assert!(
        errors.contains("destinations.fedi: give its domain, or domain in [federation]"),
        "{errors}"
    );
}

#[test]
fn a_destination_of_a_kind_that_inherits_nothing_takes_nothing() {
    let config = loaded(&format!(
        "{FEDERATION}\n[destinations.fedi]\nkind = \"discord\"\ntoken = {{ secret = \"T\" }}\n"
    ));
    assert!(config.is_err_and(|errors| !errors.contains("[federation]")));
}

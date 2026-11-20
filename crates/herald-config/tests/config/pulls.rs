//! `[sources.*]` of kind `herald`: each joined to the peer it names, or
//! pinning keys of its own no peer pins and none revoked.

use herald_config::{Config, load};
use herald_core::{Kind, SourceName};
use herald_kinds::Polling;
use herald_peers::multibase::ed25519_public;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn loaded(rest: &str) -> Result<Config, String> {
    let text = format!(
        "version = 1\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"HERALD_STORE_KEY\" }}\n[peers.upstream]\nurl = \"https://herald.example/h\"\nkeys = [\"{}\"]\n{rest}",
        key(1)
    );
    load("herald.toml", &text, &nothing, &crate::common::kinds())
        .map_err(|errors| errors.to_string())
}

fn key(byte: u8) -> String {
    ed25519_public(&[byte; 32])
}

fn api(config: &Config, name: &str) -> String {
    let Polling::Pull(pull) = config.sources[&SourceName::new(name)].poller(None, false) else {
        panic!("a pull");
    };
    pull.api().to_owned()
}

#[test]
fn a_source_naming_a_peer_pulls_the_feed_served_where_the_peer_is() {
    let config =
        loaded("[sources.up]\nkind = \"herald\"\npeer = \"upstream\"\nfeed = \"releases\"\n")
            .unwrap();
    assert_eq!(
        api(&config, "up"),
        "https://herald.example/h/herald/v1/feeds/releases"
    );
    assert_eq!(
        loaded("[sources.up]\nkind = \"herald\"\npeer = \"elsewhere\"\nfeed = \"releases\"\n")
            .unwrap_err(),
        "herald.toml:8:1 sources.up.peer: there is no peer \"elsewhere\" in [peers]; `herald peer add <url>` pins one"
    );
}

#[test]
fn a_static_log_is_read_from_a_peer_whose_keys_rotate_by_hand_only() {
    let source = "[sources.up]\nkind = \"herald\"\npeer = \"upstream\"\nfeed = \"releases\"\nstatic = true\n";
    assert!(loaded(source).is_ok());
    let refused = loaded(&format!("rotation = \"signed\"\n{source}")).unwrap_err();
    assert!(
            refused.contains("sources.up.static: a static log is served with no signed answer, which a signed rotation is read from; set rotation = \"manual\" in [peers.upstream], or read the peer's herald server"),
            "{refused}"
        );
}

#[test]
fn a_source_pinning_its_own_keys_pins_none_a_peer_pins_or_revoked() {
    let url = "url = \"https://other.example/herald/v1/feeds/releases\"";
    let config = loaded(&format!(
        "[sources.other]\nkind = \"herald\"\n{url}\nkeys = [\"{}\"]\n",
        key(2)
    ))
    .unwrap();
    assert_eq!(
        api(&config, "other"),
        "https://other.example/herald/v1/feeds/releases"
    );
    let pinned = loaded(&format!(
        "[sources.other]\nkind = \"herald\"\n{url}\nkeys = [\"{}\"]\n",
        key(1)
    ))
    .unwrap_err();
    assert_eq!(
        pinned,
        format!(
            "herald.toml:8:1 sources.other.keys: the key {} is pinned for the peer upstream; name it as peer = \"upstream\" in place of url and keys",
            key(1)
        )
    );
    let revoked = loaded(&format!(
            "[federation]\nrevoked = [\"{}\"]\n[sources.other]\nkind = \"herald\"\n{url}\nkeys = [\"{}\"]\n",
            key(3),
            key(3)
        ))
        .unwrap_err();
    assert!(
        revoked.contains(&format!(
            "sources.other.keys: the key {} is revoked in [federation] revoked",
            key(3)
        )),
        "{revoked}"
    );
}

#[test]
fn a_route_from_a_peer_s_feed_takes_every_kind_the_config_knows() {
    let config = loaded(
        "[sources.up]\nkind = \"herald\"\npeer = \"upstream\"\nfeed = \"releases\"\n[destinations.feed]\nkind = \"herald\"\n[federation]\ndomain = \"herald.self\"\nkeys = [{ secret = \"HERALD_KEY\" }]\n[[routes]]\nid = \"relay\"\nsources = [\"up\"]\ndestination = \"feed\"\nversions = \"*\"\n",
    );
    let refused = config.unwrap_err();
    assert!(refused.contains("routes[0].versions"), "{refused}");
    let config = loaded(
            "[sources.up]\nkind = \"herald\"\npeer = \"upstream\"\nfeed = \"releases\"\n[destinations.feed]\nkind = \"herald\"\n[federation]\ndomain = \"herald.self\"\nkeys = [{ secret = \"HERALD_KEY\" }]\n[[routes]]\nid = \"relay\"\nsources = [\"up\"]\ndestination = \"feed\"\nkinds = [\"release\", \"feed-entry\", \"run\"]\n",
        )
        .unwrap();
    assert_eq!(
        config.routes[0].route.filter.kinds,
        [Kind::RELEASE, Kind::FEED_ENTRY, Kind::RUN]
    );
}

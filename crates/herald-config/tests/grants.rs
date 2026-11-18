//! Grants: what a peer may do with a herald source, written both on the
//! peer and on the source, which must say the same.

mod common;

#[cfg(test)]
mod grants {
    use std::collections::BTreeSet;

    use herald_config::{Config, load};
    use herald_core::{Action, Grant, SourceName};
    use herald_peers::multibase::ed25519_public;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn loaded(peer_grants: &str, source_grants: &str) -> Result<Config, String> {
        let text = format!(
            "version = 1\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"HERALD_STORE_KEY\" }}\n\
             [peers.org]\nurl = \"https://herald.example\"\nkeys = [\"{}\"]\n{peer_grants}\n\
             [sources.up]\nkind = \"herald\"\npeer = \"org\"\nfeed = \"releases\"\n{source_grants}\n\
             [sources.forge]\nkind = \"github\"\nrepository = \"acme/gate\"\n",
            ed25519_public(&[1; 32])
        );
        load("herald.toml", &text, &nothing, &crate::common::kinds())
            .map_err(|errors| errors.to_string())
    }

    const PEER_PUSH: &str = "grants = [{ resource = \"sources.up\", actions = [\"push\"], until = \"2027-01-01T00:00:00Z\", namespaces = [\"github.com/1\"] }]";
    const SOURCE_PUSH: &str = "peers = { org = { actions = [\"push\"], until = \"2027-01-01T00:00:00Z\", namespaces = [\"github.com/1\"] } }";

    #[test]
    fn a_grant_both_places_name_alike_is_read_onto_its_peer() {
        let config = loaded(PEER_PUSH, SOURCE_PUSH).unwrap();
        assert_eq!(
            config.peers["org"].grants[&SourceName::new("up")],
            Grant {
                actions: BTreeSet::from([Action::Push]),
                until: Some("2027-01-01T00:00:00Z".parse().unwrap()),
                namespaces: BTreeSet::from(["github.com/1".to_owned()]),
            }
        );
        assert!(loaded("", "").unwrap().peers["org"].grants.is_empty());
    }

    #[test]
    fn a_grant_may_end_at_a_toml_datetime_on_either_side() {
        let config = loaded(
            &PEER_PUSH.replace("\"2027-01-01T00:00:00Z\"", "2027-01-01T01:00:00+01:00"),
            &SOURCE_PUSH.replace("\"2027-01-01T00:00:00Z\"", "2027-01-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(
            config.peers["org"].grants[&SourceName::new("up")].until,
            Some("2027-01-01T00:00:00Z".parse().unwrap())
        );
        let local = loaded(
            &PEER_PUSH.replace("\"2027-01-01T00:00:00Z\"", "2027-01-01T00:00:00"),
            SOURCE_PUSH,
        )
        .unwrap_err();
        assert!(
            local.contains("2027-01-01T00:00:00 is no instant; until is a time with its offset"),
            "{local}"
        );
        let numbered = loaded(
            PEER_PUSH.replace("\"2027-01-01T00:00:00Z\"", "5").as_str(),
            SOURCE_PUSH,
        )
        .unwrap_err();
        assert!(
            numbered.contains("until is a time with its offset"),
            "{numbered}"
        );
        let endless = loaded(
            &PEER_PUSH.replace(", until = \"2027-01-01T00:00:00Z\"", ""),
            &SOURCE_PUSH.replace(", until = \"2027-01-01T00:00:00Z\"", ""),
        )
        .unwrap();
        assert_eq!(
            endless.peers["org"].grants[&SourceName::new("up")].until,
            None
        );
    }

    #[test]
    fn a_grant_only_one_place_names_is_refused_naming_the_other() {
        let peer_only = loaded(PEER_PUSH, "").unwrap_err();
        assert!(
            peer_only.contains("sources.up.peers.org must grant the same as the grant for sources.up in [peers.org]"),
            "{peer_only}"
        );
        let source_only = loaded("", SOURCE_PUSH).unwrap_err();
        assert!(
            source_only.contains("sources.up.peers.org")
                && source_only.contains("[peers.org] grants nothing on sources.up"),
            "{source_only}"
        );
    }

    #[test]
    fn grants_that_differ_in_until_or_namespaces_are_refused() {
        let later = SOURCE_PUSH.replace("2027", "2028");
        assert!(
            loaded(PEER_PUSH, &later)
                .unwrap_err()
                .contains("must grant the same")
        );
        let wider = SOURCE_PUSH.replace("\"github.com/1\"", "\"github.com/1\", \"github.com/2\"");
        assert!(
            loaded(PEER_PUSH, &wider)
                .unwrap_err()
                .contains("must grant the same")
        );
    }

    #[test]
    fn a_grant_naming_no_herald_source_or_no_action_is_refused() {
        let missing = loaded(
            "grants = [{ resource = \"sources.gone\", actions = [\"push\"] }]",
            "",
        )
        .unwrap_err();
        assert!(
            missing.contains("there is no source sources.gone to grant"),
            "{missing}"
        );
        let forge = loaded(
            "grants = [{ resource = \"sources.forge\", actions = [\"push\"] }]",
            "",
        )
        .unwrap_err();
        assert!(forge.contains("sources.forge takes no grants"), "{forge}");
        let route = loaded(
            "grants = [{ resource = \"routes.up\", actions = [\"push\"] }]",
            "",
        )
        .unwrap_err();
        assert!(
            route.contains("\"routes.up\" is no resource a peer may be granted"),
            "{route}"
        );
        let idle =
            loaded("grants = [{ resource = \"sources.up\", actions = [] }]", "").unwrap_err();
        assert!(idle.contains("peers.org.grants[0].actions"), "{idle}");
        let twice = loaded(
            "grants = [{ resource = \"sources.up\", actions = [\"push\"] }, { resource = \"sources.up\", actions = [\"push\"] }]",
            "peers = { org = { actions = [\"push\"] } }",
        )
        .unwrap_err();
        assert!(twice.contains("sources.up is granted twice"), "{twice}");
        let pull = loaded(
            "grants = [{ resource = \"sources.up\", actions = [\"pull\"] }]",
            "",
        )
        .unwrap_err();
        assert!(pull.contains("pull"), "{pull}");
    }

    #[test]
    fn a_source_s_grant_is_checked_as_its_options_are() {
        let idle = loaded("", "peers = { org = { actions = [] } }").unwrap_err();
        assert!(idle.contains("peers.org grants no action"), "{idle}");
        let wide = loaded(
            "",
            "peers = { org = { actions = [\"push\"], namespaces = [\"herald/x\"] } }",
        )
        .unwrap_err();
        assert!(
            wide.contains("is not a namespace a peer may be granted"),
            "{wide}"
        );
        let named = loaded("", "peers = { Org = { actions = [\"push\"] } }").unwrap_err();
        assert!(named.contains("\"Org\" is not a peer's name"), "{named}");
    }
}

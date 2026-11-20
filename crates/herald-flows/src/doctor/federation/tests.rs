//! Tests of what doctor's federation section counts as its own.

use super::*;

fn bare() -> Federation {
    Federation {
        domain: None,
        keys: Vec::new(),
        revoked: Vec::new(),
        peers: Vec::new(),
        feeds: Vec::new(),
        pushes: Vec::new(),
        sources: Vec::new(),
        grants: Vec::new(),
        runs: Vec::new(),
        keep_audit: SignedDuration::ZERO,
        pages: None,
        routes: Vec::new(),
        gaps: Vec::new(),
    }
}

fn peer(name: &str) -> PeerShown {
    PeerShown {
        name: name.to_owned(),
        url: String::new(),
        keys: Vec::new(),
        rotation: "manual",
        rate: String::new(),
        most: String::new(),
        private: false,
        reached: None,
    }
}

fn run(app: &str) -> RunShown {
    RunShown {
        app: app.to_owned(),
        issuer: "https://issuer.example".to_owned(),
        audience: "https://herald.example".to_owned(),
        peers: vec!["plugin-ci".to_owned()],
    }
}

#[test]
fn a_config_takes_part_among_heralds_by_any_one_of_its_parts() {
    assert!(!bare().any());
    let parts: [fn(&mut Federation); 8] = [
        |shown| shown.domain = Some("herald.example".to_owned()),
        |shown| {
            shown.keys.push(OwnKey {
                secret: "KEY".to_owned(),
                shown: None,
            });
        },
        |shown| shown.peers.push(peer("org")),
        |shown| {
            shown.feeds.push(FeedShown {
                name: "feed".to_owned(),
                url: String::new(),
                served: false,
            });
        },
        |shown| {
            shown.pushes.push(PushShown {
                name: "mirror".to_owned(),
                peer: "org".to_owned(),
                inbox: String::new(),
                private: false,
                id_token: false,
            });
        },
        |shown| {
            shown.sources.push(SourceShown {
                name: "up".to_owned(),
                pulls: None,
                pushed: true,
                stale_after: None,
            });
        },
        |shown| shown.runs.push(run("ci")),
        |shown| {
            shown.pages = Some(herald_config::Pages {
                url: "https://owner.example".to_owned(),
                base: String::new(),
                branch: None,
                repository: None,
                token: None,
                api: None,
                artifact: Some("_site".to_owned()),
                refresh: SignedDuration::ZERO,
            });
        },
    ];
    for part in parts {
        let mut shown = bare();
        part(&mut shown);
        assert!(shown.any(), "{shown:?}");
    }
}

#[test]
fn a_gap_is_federation_s_where_its_key_is_of_one_of_its_parts_on_a_boundary() {
    let mut shown = bare();
    shown.peers.push(peer("org"));
    shown.feeds.push(FeedShown {
        name: "feed".to_owned(),
        url: String::new(),
        served: false,
    });
    shown.pushes.push(PushShown {
        name: "mirror".to_owned(),
        peer: "org".to_owned(),
        inbox: String::new(),
        private: false,
        id_token: true,
    });
    shown.sources.push(SourceShown {
        name: "up".to_owned(),
        pulls: None,
        pushed: true,
        stale_after: None,
    });
    shown.routes.push(2);
    shown.runs.push(run("ci"));
    for owned in [
        "federation",
        "federation.keys[0]",
        "peers.org",
        "peers.org.rotation",
        "destinations.feed",
        "destinations.mirror.peer",
        "sources.up",
        "sources.up.peers",
        "sources.up[0]",
        "routes[2]",
        "routes[2].private",
        "apps.ci",
        "apps.ci.oidc[0].claims",
    ] {
        assert!(shown.owns(owned), "{owned}");
    }
    for other in [
        "federationx",
        "peers.organic",
        "destinations.feeds",
        "destinations.mirrored",
        "sources.upstream",
        "store.kind",
        "routes[0].destination",
        "routes[20].private",
        "apps.cis",
    ] {
        assert!(!shown.owns(other), "{other}");
    }
}

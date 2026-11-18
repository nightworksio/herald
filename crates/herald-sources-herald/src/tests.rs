use std::num::NonZeroUsize;

use herald_kinds::{Instance, Peering, Polling, SourceConfig};
use herald_peers::multibase::ed25519_public;

use super::*;

fn file() -> HeraldFile {
    HeraldFile {
        peer: Some("upstream".to_owned()),
        feed: Some("releases".to_owned()),
        ..HeraldFile::default()
    }
}

fn pinned() -> HeraldFile {
    HeraldFile {
        url: Some("https://herald.example.net/h/herald/v1/feeds/releases".to_owned()),
        keys: vec![ed25519_public(&[7; 32])],
        ..HeraldFile::default()
    }
}

fn refused(file: HeraldFile) -> String {
    let fault = HeraldKind::checked(file).unwrap_err();
    match fault.key {
        Some(key) => format!("{key}: {}", fault.message),
        None => fault.message,
    }
}

#[test]
fn a_source_pulls_a_peer_s_feed_once_the_config_joins_it_to_the_peer() {
    let options = HeraldKind::checked(file()).unwrap();
    assert_eq!(options.kind(), "herald");
    assert_eq!(options.token(), None);
    assert_eq!(options.kinds(), []);
    assert!(options.any_kind());
    assert!(options.origins());
    assert_eq!(options.backfill(), 0);
    assert_eq!(options.peering(), Some(Peering::Peer("upstream")));
    let Polling::Pull(unjoined) = options.poller(None, false) else {
        panic!("a pull");
    };
    assert_eq!(unjoined.api(), "");
    let joined = options.joined("https://herald.example.net/h").unwrap();
    let Polling::Pull(pull) = joined.poller(None, false) else {
        panic!("a pull");
    };
    assert_eq!(
        pull.api(),
        "https://herald.example.net/h/herald/v1/feeds/releases"
    );
    assert_eq!(pull.remembered(), HeraldOptions::PAGES * 80);
    assert_eq!(joined.peering(), Some(Peering::Peer("upstream")));
    assert_eq!(pull.peering(), Peering::Peer("upstream"));
}

#[test]
fn a_source_pinning_its_own_keys_names_the_instance_by_its_domain() {
    let options = HeraldKind::checked(HeraldFile {
        backfill: 3,
        pages: NonZeroUsize::new(2),
        ..pinned()
    })
    .unwrap();
    assert_eq!(
        options.peering(),
        Some(Peering::Pinned(Instance {
            name: "herald.example.net",
            url: "https://herald.example.net/h",
            domain: "herald.example.net",
            keys: &[[7; 32]],
        }))
    );
    assert!(options.joined("https://other.example").is_none());
    assert_eq!(options.backfill(), 3);
    let Polling::Pull(pull) = options.poller(None, false) else {
        panic!("a pull");
    };
    assert_eq!(
        pull.api(),
        "https://herald.example.net/h/herald/v1/feeds/releases"
    );
    assert_eq!(pull.remembered(), 160);
    assert_eq!(Some(pull.peering()), options.peering());
}

#[test]
fn a_source_names_a_peer_with_its_feed_or_a_url_with_its_keys() {
    let either = "name a peer of [peers] with its feed, as peer = \"<name>\" and feed = \"<feed>\", or a feed's url with the keys pinned for it, not both";
    assert_eq!(refused(HeraldFile::default()), either);
    assert_eq!(
        refused(HeraldFile {
            feed: None,
            ..file()
        }),
        either
    );
    assert_eq!(
        refused(HeraldFile {
            url: pinned().url,
            ..file()
        }),
        either
    );
    assert_eq!(
        refused(HeraldFile {
            peer: Some("Upstream".to_owned()),
            ..file()
        }),
        "peer: \"Upstream\" is not a peer's name; name a table of [peers]"
    );
    assert_eq!(
        refused(HeraldFile {
            feed: Some(String::new()),
            ..file()
        }),
        "feed: name the peer's feed, as its discovery document lists it"
    );
    assert_eq!(
        refused(HeraldFile {
            keys: pinned().keys,
            ..file()
        }),
        "keys: a peer's keys are pinned in [peers]; leave keys out, or name a url in place of the peer"
    );
    assert!(
        refused(HeraldFile {
            url: Some("https://herald.example.net/feed.json".to_owned()),
            ..pinned()
        })
        .starts_with("url: \"https://herald.example.net/feed.json\" is not a feed's URL")
    );
}

#[test]
fn a_url_s_keys_are_ed25519_each_once_and_at_least_one() {
    assert_eq!(
        refused(HeraldFile {
            keys: Vec::new(),
            ..pinned()
        }),
        "keys: pin at least one of the instance's keys"
    );
    let key = ed25519_public(&[7; 32]);
    assert_eq!(
        refused(HeraldFile {
            keys: vec![key.clone(), key.clone()],
            ..pinned()
        }),
        format!("keys: \"{key}\" is pinned twice; pin each key once")
    );
    assert!(
        refused(HeraldFile {
            keys: vec!["z6MkNot".to_owned()],
            ..pinned()
        })
        .starts_with("keys: \"z6MkNot\" is not an Ed25519 Multikey")
    );
}

#[test]
fn pages_backfill_and_namespaces_keep_within_their_bounds() {
    let pages = |pages: usize| HeraldFile {
        pages: NonZeroUsize::new(pages),
        ..file()
    };
    assert!(HeraldKind::checked(pages(HeraldOptions::PAGES_ALLOWED)).is_ok());
    assert_eq!(
        refused(pages(HeraldOptions::PAGES_ALLOWED + 1)),
        "pages: pages is at most 20; lower it"
    );
    let tombstones = |tombstones: usize| HeraldFile {
        tombstones: NonZeroUsize::new(tombstones),
        ..file()
    };
    assert_eq!(
        HeraldKind::checked(file()).map(|options| options.tombstones),
        Ok(HeraldOptions::TOMBSTONES)
    );
    assert_eq!(
        HeraldKind::checked(tombstones(HeraldOptions::TOMBSTONES_ALLOWED))
            .map(|options| options.tombstones),
        Ok(4000)
    );
    assert_eq!(
        refused(tombstones(HeraldOptions::TOMBSTONES_ALLOWED + 1)),
        "tombstones: tombstones is at most 4000; lower it"
    );
    let peer = |tombstones: usize, peer: usize| HeraldFile {
        tombstones: NonZeroUsize::new(tombstones),
        peer_tombstones: NonZeroUsize::new(peer),
        ..file()
    };
    let kept = |file: HeraldFile| {
        HeraldKind::checked(file).map(|options| (options.tombstones, options.peer_tombstones))
    };
    assert_eq!(kept(file()), Ok((1000, 250)));
    assert_eq!(kept(tombstones(100)), Ok((100, 100)));
    assert_eq!(kept(peer(100, 100)), Ok((100, 100)));
    let share = |share: usize| HeraldFile {
        peer_share: NonZeroUsize::new(share),
        ..file()
    };
    assert_eq!(
        HeraldKind::checked(file()).map(|options| options.peer_share),
        Ok(1000)
    );
    assert_eq!(
        HeraldKind::checked(share(4000)).map(|options| options.peer_share),
        Ok(4000)
    );
    assert_eq!(
        HeraldKind::checked(share(30)).map(|options| options.share()),
        Ok(Some(30))
    );
    assert_eq!(
        refused(share(4001)),
        "peer_share: peer_share is at most 4000; lower it"
    );
    assert_eq!(
        refused(peer(100, 101)),
        "peer_tombstones: peer_tombstones is at most tombstones, 100; lower it or raise tombstones"
    );
    let backfill = |backfill: usize| HeraldFile {
        backfill,
        ..pages(2)
    };
    assert!(HeraldKind::checked(backfill(160)).is_ok());
    assert_eq!(
        refused(backfill(161)),
        "backfill: backfill is at most 160, the 2 pages of 80 one pull reads; lower it or raise pages"
    );
    assert!(
        HeraldKind::checked(HeraldFile {
            namespaces: vec!["github.com/1".to_owned(), "heraldry.example".to_owned()],
            ..file()
        })
        .is_ok()
    );
    for namespace in ["", "github.com/", "herald", "herald/other"] {
        let said = refused(HeraldFile {
            namespaces: vec![namespace.to_owned()],
            ..file()
        });
        assert!(
            said.starts_with(&format!(
                "namespaces: \"{namespace}\" is not a namespace a peer may be granted"
            )),
            "{said}"
        );
    }
}

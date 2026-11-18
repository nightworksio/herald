//! `herald peer add --update` for a peer whose rotation is signed: the key
//! set the store keeps it handed over to pinned without asking.

use herald_compose::{Host, Hosting, ServingOn, Through};
use herald_config::Uses;
use herald_core::PeerKeys;
use herald_ports::{Keyring, Published as _, Store};

use super::*;

struct Keep(PeerKeys);

impl<H: Host> Through<H> for Keep {
    type Done = ();

    async fn through<S: Store, K: Keyring>(self, serving: &ServingOn<'_, H, S, K>, _: &str) {
        let kept = serving.ports.store.published().unwrap();
        kept.put_peer_keys("org", &self.0, None).await.unwrap();
    }
}

pub(in crate::tests) fn kept(setup: &Setup, world: &World, held: PeerKeys) {
    let config = crate::read(&path_of(setup)).unwrap();
    let connect =
        |_: herald_ports::Base<'_>, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let waiting = |_: &str, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let connector = Connector::new(&connect, &waiting);
    on_tokio(async {
        let composed = crate::stores::composed(
            &config,
            &setup.vars,
            &connector,
            Hosting::Actions,
            Uses::Polls,
            "test",
        )
        .await
        .unwrap();
        composed.run(Keep(held)).await;
    });
}

pub(in crate::tests) fn signed() -> String {
    format!(
        "[peers.org]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\"]\nrotation = \"signed\"\n",
        multikey(OLDER)
    )
}

pub(in crate::tests) fn held(pinned: &str, keys: &[&str]) -> PeerKeys {
    PeerKeys {
        pinned: vec![multikey(pinned)],
        sequence: 4,
        keys: keys.iter().map(|seed| multikey(seed)).collect(),
        read: jiff::Timestamp::UNIX_EPOCH,
        refresh: 3600,
    }
}

#[test]
fn the_set_a_signed_peer_handed_over_to_is_pinned_without_asking() {
    let (setup, world) = peered(&signed(), &[NEWER, OLDER]);
    kept(&setup, &world, held(OLDER, &[NEWER, OLDER]));
    let (code, out) = answered(
        &setup,
        &world,
        &[DOMAIN, "--name", "org", "--update", "--print"],
        "",
    );
    assert_eq!(code, DONE, "{out}");
    assert!(out.starts_with("The peer org handed over to 2 keys under sequence 4, each set proved by a key trusted for it:\n"), "{out}");
    assert!(out.contains("[peers.org]"), "{out}");
    let (code, out) = answered(&setup, &world, &[DOMAIN, "--name", "org", "--update"], "");
    assert_eq!(code, DONE, "{out}");
    let config = crate::read(&path_of(&setup)).unwrap();
    assert_eq!(config.peers["org"].keys, [public(NEWER), public(OLDER)]);
}

#[test]
fn a_set_kept_over_other_keys_or_the_same_keys_falls_back_to_the_discovery_document() {
    for kept_set in [held(NEWER, &[NEWER]), held(OLDER, &[OLDER])] {
        let (setup, world) = peered(&signed(), &[NEWER, OLDER]);
        kept(&setup, &world, kept_set);
        let (code, out) = answered(
            &setup,
            &world,
            &[DOMAIN, "--name", "org", "--update"],
            "y\n",
        );
        assert_eq!(code, DONE, "{out}");
        assert!(out.contains("lists 2 keys under sequence 3"), "{out}");
    }
    let (setup, world) = peered(&signed(), &[NEWER, OLDER]);
    let (code, out) = answered(
        &setup,
        &world,
        &[DOMAIN, "--name", "org", "--update"],
        "n\n",
    );
    assert_eq!(code, STOPPED, "{out}");
}

#[test]
fn a_store_that_keeps_no_peer_s_set_says_so() {
    let marked = CONFIG.replace(
        "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
        "kind = \"markers\"\n[markers]\nkey = { secret = \"HERALD_STORE_KEY\" }",
    );
    let setup = setup(&format!("{marked}{}", signed()));
    let world = World::default();
    world.state.lock().unwrap().peer = Some(served(&[NEWER, OLDER], 3));
    let path = path_of(&setup);
    let config = crate::read(&path).unwrap();
    let crate::cli::Command::Read(Reading::Peer(asked)) = crate::cli::Cli::try_parse_from([
        "herald", "peer", "add", DOMAIN, "--name", "org", "--update",
    ])
    .unwrap()
    .command
    else {
        panic!("not a peer command");
    };
    let connect =
        |_: herald_ports::Base<'_>, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let waiting = |_: &str, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let connector = Connector::new(&connect, &waiting);
    let refused = on_tokio(crate::peers::peering(
        &config,
        &path,
        &asked,
        (&setup.vars, &connector),
        &mut "".as_bytes(),
        &mut Vec::new(),
    ))
    .unwrap_err();
    assert!(refused.contains("key sets"), "{refused}");
}

use super::world::*;
use crate::*;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_crypto::RingSigner;
use herald_peers::answer::{CONTENT_TYPE, demanded};
use herald_peers::digest::{CONTENT_DIGEST, content_digest};
use herald_peers::discovery::Instance;
use herald_peers::keys::{Key, KeySet, fingerprint, method};
use herald_peers::message::Covered;
use herald_peers::multibase::ed25519_public;
use herald_ports::{Algorithm, Response, Secret, Signer as _};

const PKCS8_ED25519: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];
pub(super) const NEWER: &str =
    include_str!("../../../herald-crypto/fixtures/ed25519-rfc9421-seed.test.hex");
pub(super) const OLDER: &str =
    include_str!("../../../herald-crypto/fixtures/ed25519-w3c-seed.test.hex");
const DOMAIN: &str = "herald.peer.example";
const DISCOVERY: &str = "https://herald.peer.example/.well-known/herald";

pub(super) fn seeded(seed: &str) -> Secret {
    let der = [
        &PKCS8_ED25519[..],
        &herald_core::unhex(seed.trim()).unwrap(),
    ]
    .concat();
    Secret::new(
        "PEER",
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(der)
        ),
    )
}

fn public(seed: &str) -> Key {
    let spki = pollster::block_on(RingSigner::default().public(Algorithm::Ed25519, &seeded(seed)))
        .unwrap();
    spki[spki.len() - 32..].try_into().unwrap()
}

fn multikey(seed: &str) -> String {
    ed25519_public(&public(seed))
}

/// The peer's discovery document listing the keys of `seeds`, its answer
/// signed by the first.
fn served(seeds: &[&str], sequence: u64) -> Response {
    let set = KeySet {
        sequence,
        refresh: 86_400,
        keys: seeds.iter().map(|seed| public(seed)).collect(),
    };
    let instance = Instance {
        domain: DOMAIN,
        base: "",
        software: "herald",
        version: "1.0.0",
        repository: "https://github.com/nightworksio/herald",
    };
    let body = instance.document(&set.document(DOMAIN), &[]).into_bytes();
    let mut headers = vec![
        (CONTENT_TYPE.to_owned(), "application/json".to_owned()),
        (CONTENT_DIGEST.to_owned(), content_digest(&body)),
    ];
    let now = jiff::Timestamp::now().as_second();
    let covered = Covered {
        components: demanded().to_vec(),
        created: now,
        expires: now + 300,
        nonce: None,
        keyid: method(DOMAIN, &set.keys[0]),
    };
    let message = herald_peers::message::Message {
        method: "GET",
        url: DISCOVERY,
        status: Some(200),
        fields: &headers,
    };
    let base = covered.base(&message).unwrap();
    let signature = pollster::block_on(RingSigner::default().sign(
        Algorithm::Ed25519,
        &seeded(seeds[0]),
        &base,
    ))
    .unwrap();
    headers.extend(covered.fields(&signature));
    Response {
        status: 200,
        headers,
        body,
    }
}

fn peered(peers: &str, seeds: &[&str]) -> (Setup, World) {
    let setup = setup(&format!("{CONFIG}{peers}"));
    let world = World::default();
    world.state.lock().unwrap().peer = Some(served(seeds, 3));
    (setup, world)
}

fn config_of(setup: &Setup) -> String {
    std::fs::read_to_string(setup.directory.path().join("herald.toml")).unwrap()
}

fn path_of(setup: &Setup) -> String {
    setup
        .directory
        .path()
        .join("herald.toml")
        .display()
        .to_string()
}

/// `herald peer add` as `args` asks, the operator answering `answer`.
fn answered(setup: &Setup, world: &World, args: &[&str], answer: &str) -> (u8, String) {
    let path = path_of(setup);
    let config = crate::read(&path).unwrap();
    let mut all = vec!["herald", "peer", "add"];
    all.extend(args);
    let crate::cli::Command::Read(Reading::Peer(asked)) =
        crate::cli::Cli::try_parse_from(all).unwrap().command
    else {
        panic!("not a peer command");
    };
    let connect =
        |_: herald_ports::Base<'_>, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let waiting = |_: &str, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let connector = Connector::new(&connect, &waiting);
    let mut out = Vec::new();
    let mut answers = answer.as_bytes();
    let code = on_tokio(crate::peers::peering(
        &config,
        &path,
        &asked,
        (&setup.vars, &connector),
        &mut answers,
        &mut out,
    ))
    .unwrap();
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn a_peer_s_keys_are_shown_and_pinned_once_the_operator_confirms_them() {
    let (setup, world) = peered("", &[NEWER, OLDER]);
    let (code, out) = answered(&setup, &world, &["herald.peer.example"], "Yes\n");
    assert_eq!(code, DONE, "{out}");
    let first = format!(
        "herald.peer.example lists 2 keys under sequence 3, its answer signed by {}:
",
        fingerprint(&public(NEWER))
    );
    assert!(out.starts_with(&first), "{out}");
    for seed in [NEWER, OLDER] {
        assert!(
            out.contains(&format!(
                "  {} {}\n",
                multikey(seed),
                fingerprint(&public(seed))
            )),
            "{out}"
        );
    }
    assert!(out.contains("Compare each fingerprint with one the peer's operator gave you. Pin these 2 keys for the peer herald-peer-example in"), "{out}");
    assert!(
        out.ends_with(&format!(
            "[y/N] Pinned 2 keys for the peer herald-peer-example in {}.\n",
            path_of(&setup)
        )),
        "{out}"
    );
    let config = crate::read(&path_of(&setup)).unwrap();
    let peer = &config.peers["herald-peer-example"];
    assert_eq!(peer.url, "https://herald.peer.example");
    assert_eq!(peer.keys, [public(NEWER), public(OLDER)]);
    assert!(
        config_of(&setup).starts_with(CONFIG),
        "every other line is kept"
    );
}

#[test]
fn nothing_is_pinned_unless_the_operator_says_yes() {
    let (setup, world) = peered("", &[NEWER]);
    for answer in ["n\n", "\n", ""] {
        let (code, out) = answered(&setup, &world, &["herald.peer.example"], answer);
        assert_eq!(code, STOPPED);
        assert!(out.ends_with("Nothing was pinned.\n"), "{out}");
        assert_eq!(config_of(&setup), CONFIG);
    }
}

#[test]
fn keys_named_on_the_command_line_are_pinned_without_asking_and_only_they() {
    let (setup, world) = peered("", &[NEWER, OLDER]);
    let older = fingerprint(&public(OLDER));
    let run = herald(
        &setup,
        &world,
        &[
            "peer",
            "add",
            "https://herald.peer.example/",
            "--name",
            "upstream",
            "--key",
            &older,
        ],
    );
    assert_eq!(run.code, DONE, "{}{}", run.out, run.err);
    assert!(!run.out.contains("[y/N]"), "{}", run.out);
    let config = crate::read(&path_of(&setup)).unwrap();
    assert_eq!(config.peers["upstream"].keys, [public(OLDER)]);
    let unknown = herald(
        &setup,
        &world,
        &[
            "peer",
            "add",
            DOMAIN,
            "--name",
            "other",
            "--key",
            "z6MkNotListed",
        ],
    );
    assert_eq!(unknown.code, STOPPED);
    assert!(
        unknown
            .err
            .contains("the peer lists no key z6MkNotListed, so nothing was pinned"),
        "{}",
        unknown.err
    );
    let newer = multikey(NEWER);
    let twice = [
        "peer", "add", DOMAIN, "--name", "other", "--key", &newer, "--key", &newer,
    ];
    let by_multikey = herald(&setup, &world, &twice);
    assert_eq!(by_multikey.code, DONE, "{}", by_multikey.err);
    let config = crate::read(&path_of(&setup)).unwrap();
    assert_eq!(config.peers["other"].keys, [public(NEWER)]);
    let third = ["peer", "add", DOMAIN, "--name", "third", "--key", &older];
    let taken = herald(&setup, &world, &third);
    assert!(
        taken
            .err
            .contains("the peer upstream pins one of these keys already"),
        "{}",
        taken.err
    );
    let again = [
        "peer", "add", DOMAIN, "--name", "upstream", "--update", "--key", &older,
    ];
    let updated = herald(&setup, &world, &again);
    assert_eq!(updated.code, DONE, "{}", updated.err);
}

#[test]
fn print_shows_the_table_and_writes_nothing() {
    let (setup, world) = peered("", &[NEWER]);
    let run = herald(&setup, &world, &["peer", "add", DOMAIN, "--print"]);
    assert_eq!(run.code, DONE, "{}", run.err);
    assert!(
        run.out.ends_with(&format!(
            "[peers.herald-peer-example]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\"]\n",
            multikey(NEWER)
        )),
        "{}",
        run.out
    );
    assert_eq!(config_of(&setup), CONFIG);
}

#[test]
fn a_peer_already_pinned_takes_new_keys_only_on_update_keeping_its_other_settings() {
    let pinned = format!(
        "[peers.org]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\"]\nrate = \"60/m\"\n",
        multikey(OLDER)
    );
    let (setup, world) = peered(&pinned, &[NEWER, OLDER]);
    let again = herald(&setup, &world, &["peer", "add", DOMAIN, "--name", "org"]);
    assert_eq!(again.code, STOPPED);
    assert!(
        again
            .err
            .contains("a peer named org is pinned already; give --update"),
        "{}",
        again.err
    );
    let (code, out) = answered(
        &setup,
        &world,
        &[DOMAIN, "--name", "org", "--update"],
        "y\n",
    );
    assert_eq!(code, DONE, "{out}");
    assert!(
        out.contains(&format!("{} (pinned)", fingerprint(&public(OLDER)))),
        "{out}"
    );
    let config = crate::read(&path_of(&setup)).unwrap();
    assert_eq!(config.peers["org"].keys, [public(NEWER), public(OLDER)]);
    assert_eq!(config.peers["org"].rate.to_string(), "60/m");
    let missing = herald(
        &setup,
        &world,
        &["peer", "add", DOMAIN, "--name", "nobody", "--update"],
    );
    assert!(
        missing
            .err
            .contains("no peer named nobody is pinned to update"),
        "{}",
        missing.err
    );
}

#[test]
fn a_revoked_key_is_never_pinned() {
    let revoked = format!("[federation]\nrevoked = [\"{}\"]\n", multikey(OLDER));
    let (setup, world) = peered(&revoked, &[NEWER, OLDER]);
    let named = herald(
        &setup,
        &world,
        &["peer", "add", DOMAIN, "--key", &multikey(OLDER)],
    );
    assert!(
        named
            .err
            .contains("is revoked in [federation] revoked, so nothing was pinned"),
        "{}",
        named.err
    );
    assert!(
        named
            .out
            .contains(&format!("{} (revoked)", fingerprint(&public(OLDER)))),
        "{}",
        named.out
    );
    let (code, _) = answered(&setup, &world, &[DOMAIN], "y\n");
    assert_eq!(code, DONE);
    let config = crate::read(&path_of(&setup)).unwrap();
    assert_eq!(config.peers["herald-peer-example"].keys, [public(NEWER)]);
    world.state.lock().unwrap().peer = Some(served(&[OLDER], 4));
    let only = herald(&setup, &world, &["peer", "add", DOMAIN, "--name", "lost"]);
    assert!(
        only.err
            .contains("the peer lists no key that is not revoked"),
        "{}",
        only.err
    );
}

#[test]
fn a_peer_s_address_and_name_are_checked_before_it_is_read() {
    let (setup, world) = peered("", &[NEWER]);
    let bad = herald(
        &setup,
        &world,
        &["peer", "add", "http://herald.peer.example"],
    );
    assert!(
        bad.err
            .contains("\"http://herald.peer.example\" is not a peer's address"),
        "{}",
        bad.err
    );
    let named = herald(&setup, &world, &["peer", "add", DOMAIN, "--name", "Org"]);
    assert!(
        named.err.contains("\"Org\" cannot name a peer"),
        "{}",
        named.err
    );
    world.state.lock().unwrap().peer = None;
    let unreached = herald(&setup, &world, &["peer", "add", DOMAIN]);
    assert!(
        unreached
            .err
            .contains("the discovery document could not be fetched"),
        "{}",
        unreached.err
    );
    assert!(
        world
            .state
            .lock()
            .unwrap()
            .seen
            .contains(&format!("GET {DISCOVERY}"))
    );
}

#[test]
fn list_names_each_peer_with_its_keys() {
    let (setup, world) = peered("", &[NEWER]);
    let none = herald(&setup, &world, &["peer", "list"]);
    assert_eq!(
        (none.code, none.out.as_str()),
        (DONE, "The config names no peer.\n")
    );
    let pinned = format!(
        "[peers.org]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\"]\nrotation = \"signed\"\n",
        multikey(NEWER)
    );
    let (setup, world) = peered(&pinned, &[NEWER]);
    let listed = herald(&setup, &world, &["peer", "list"]);
    assert_eq!(
        listed.out,
        format!(
            "org https://herald.peer.example (rotation signed)\n  {} {}\n",
            multikey(NEWER),
            fingerprint(&public(NEWER))
        )
    );
}

#[test]
fn test_checks_each_peer_against_its_pinned_keys_and_names_what_changed() {
    let pinned = format!(
        "[peers.org]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\"]\n",
        multikey(NEWER)
    );
    let (setup, world) = peered(&pinned, &[NEWER]);
    let held = herald(&setup, &world, &["peer", "test"]);
    assert_eq!(held.code, DONE, "{}{}", held.out, held.err);
    assert_eq!(
        held.out,
        format!(
            "org: its answer is signed by {}, a pinned key; its key set is under sequence 3\n",
            fingerprint(&public(NEWER))
        )
    );
    world.state.lock().unwrap().peer = Some(served(&[NEWER, OLDER], 4));
    let grown = herald(&setup, &world, &["peer", "test", "org"]);
    assert_eq!(grown.code, STOPPED);
    assert!(
        grown.out.contains(&format!(
            "org: lists a key not pinned:\n  {} ",
            multikey(OLDER)
        )),
        "{}",
        grown.out
    );
    assert!(grown.out.ends_with("org: pin its keys anew with `herald peer add https://herald.peer.example --name org --update`\n"), "{}", grown.out);
    world.state.lock().unwrap().peer = Some(served(&[OLDER], 5));
    let rotated = herald(&setup, &world, &["peer", "test"]);
    assert_eq!(rotated.code, STOPPED);
    assert!(
        rotated
            .out
            .contains("org: the discovery document's answer is refused: the answer is signed by"),
        "{}",
        rotated.out
    );
}

#[test]
fn test_names_a_pinned_key_the_peer_no_longer_lists() {
    let pinned = format!(
        "[peers.org]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\", \"{}\"]\n",
        multikey(NEWER),
        multikey(OLDER)
    );
    let (setup, world) = peered(&pinned, &[NEWER]);
    let shrunk = herald(&setup, &world, &["peer", "test"]);
    assert_eq!(shrunk.code, STOPPED);
    assert!(
        shrunk.out.contains(&format!(
            "org: no longer lists a pinned key:\n  {} ",
            multikey(OLDER)
        )),
        "{}",
        shrunk.out
    );
    let unknown = herald(&setup, &world, &["peer", "test", "nobody"]);
    assert!(
        unknown.err.contains("the config names no peer nobody"),
        "{}",
        unknown.err
    );
    let (bare, world) = peered("", &[NEWER]);
    let none = herald(&bare, &world, &["peer", "test"]);
    assert!(
        none.err.contains("the config names no peer to test"),
        "{}",
        none.err
    );
}

#[test]
fn revoke_takes_a_peer_or_one_key_out_and_keeps_it_revoked() {
    let pinned = format!(
        "[peers.org]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\", \"{}\"]\n",
        multikey(NEWER),
        multikey(OLDER)
    );
    let (setup, world) = peered(&pinned, &[NEWER]);
    let one = herald(
        &setup,
        &world,
        &[
            "peer",
            "revoke",
            "org",
            "--key",
            &fingerprint(&public(OLDER)),
        ],
    );
    assert_eq!(one.code, DONE, "{}", one.err);
    assert!(
        one.out.contains("Revoked 1 keys of the peer org in"),
        "{}",
        one.out
    );
    assert!(
        one.out.contains(
            "kept [peers.org] with its other keys, and added them to [federation] revoked."
        ),
        "{}",
        one.out
    );
    let config = crate::read(&path_of(&setup)).unwrap();
    assert_eq!(config.peers["org"].keys, [public(NEWER)]);
    assert_eq!(config.federation.revoked, [public(OLDER)]);
    let all = herald(&setup, &world, &["peer", "revoke", "org"]);
    assert!(all.out.contains("removed [peers.org]"), "{}", all.out);
    let config = crate::read(&path_of(&setup)).unwrap();
    assert!(config.peers.is_empty());
    assert_eq!(config.federation.revoked, [public(OLDER), public(NEWER)]);
    let gone = herald(&setup, &world, &["peer", "revoke", "org"]);
    assert!(
        gone.err.contains("the config names no peer org"),
        "{}",
        gone.err
    );
}

#[test]
fn revoke_refuses_a_key_the_peer_does_not_pin() {
    let pinned = format!(
        "[peers.org]\nurl = \"https://herald.peer.example\"\nkeys = [\"{}\"]\n",
        multikey(NEWER)
    );
    let (setup, world) = peered(&pinned, &[NEWER]);
    let run = herald(
        &setup,
        &world,
        &["peer", "revoke", "org", "--key", "z6MkElse"],
    );
    assert_eq!(run.code, STOPPED);
    assert!(
        run.err.contains("the peer org pins no key z6MkElse"),
        "{}",
        run.err
    );
}

mod doctor;
pub(super) mod handed;
mod log;

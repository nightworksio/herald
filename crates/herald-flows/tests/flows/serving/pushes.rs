//! Each `herald` source's inbox: one envelope a peer pushes, read only once
//! its signature by a key standing for the peer holds, within the peer's
//! size, rate and grant, its nonce new, and taken as a pull takes it, its
//! proof kept in the source's cursor before it is answered; every request
//! kept in its peer's audit of the day.

use std::sync::atomic::{AtomicU64, Ordering};

use herald_core::{AuditDay, Cursor, SourceName};
use herald_peers::digest::{CONTENT_DIGEST, content_digest};
use herald_peers::keys::method;
use herald_peers::message::{Covered, Message};
use herald_peers::multibase::ed25519_public;
use herald_peers::request::demanded;
use herald_ports::{Cursors as _, Published as _};
use herald_testing::fake_ed25519;
use serde_json::{Value, json};

use super::*;
use crate::pulls::{DISCOVERY, DOMAIN, OLD, envelope, proved};

mod presented;
mod rates;
mod sent;

const INBOX: &str = "/herald/v1/inbox/inbox";
const URL: &str = "https://herald.example/herald/v1/inbox/inbox";
const STRANGER: [u8; 32] = [3; 32];
const REFUSED_NOTE: &str = "A peer pushing past its rate has the nonces of at most 2400 refused requests kept in 20 minutes ([federation] refused_nonces); past them herald holds it, taking nothing of it until every request it made has expired.";

fn pushing(peer: &str, source: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "herald.example"
keys = [{{ secret = "HERALD_KEY_NEW" }}]
[peers.upstream]
url = "https://{DOMAIN}"
keys = ["{}"]
grants = [{{ resource = "sources.inbox", actions = ["push"]{peer} }}]
[destinations.releases]
kind = "herald"
[sources.inbox]
kind = "herald"
peers = {{ upstream = {{ actions = ["push"]{source} }} }}
[[routes]]
id = "mirror"
sources = ["inbox"]
destination = "releases"
private = false
"#,
        ed25519_public(&OLD)
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn world() -> World {
    World::with(pushing("", ""), Script::default())
}

/// The headers of a push of `body`, signed by `key` under the id of
/// `keyid`, with the nonce `nonce`.
fn signed(
    body: &[u8],
    (keyid, key): (&[u8; 32], &[u8; 32]),
    nonce: Option<&str>,
) -> Vec<(String, String)> {
    signed_at(URL, body, (keyid, key), nonce)
}

/// [`signed`], for a push to `url`.
fn signed_at(
    url: &str,
    body: &[u8],
    keys: (&[u8; 32], &[u8; 32]),
    nonce: Option<&str>,
) -> Vec<(String, String)> {
    signed_then(at(), url, body, keys, nonce)
}

/// [`signed_at`], signed at `then`.
fn signed_then(
    then: jiff::Timestamp,
    url: &str,
    body: &[u8],
    (keyid, key): (&[u8; 32], &[u8; 32]),
    nonce: Option<&str>,
) -> Vec<(String, String)> {
    let mut fields = vec![
        ("content-type".to_owned(), "application/json".to_owned()),
        (CONTENT_DIGEST.to_owned(), content_digest(body)),
    ];
    let now = then.as_second();
    let covered = Covered {
        components: demanded().to_vec(),
        created: now,
        expires: now + 300,
        nonce: nonce.map(str::to_owned),
        keyid: method(DOMAIN, keyid),
    };
    let message = Message {
        method: "POST",
        url,
        status: None,
        fields: &fields,
    };
    let base = covered.base(&message).unwrap();
    fields.extend(covered.fields(&fake_ed25519(key, &base)));
    fields.push(("host".to_owned(), "herald.example".to_owned()));
    fields
}

fn push(world: &World, body: &[u8], headers: &[(String, String)]) -> Answered {
    push_asking(world, None, body, headers)
}

/// [`push`], its target carrying `query`.
fn push_asking(
    world: &World,
    query: Option<&str>,
    body: &[u8],
    headers: &[(String, String)],
) -> Answered {
    let headers: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    world.answer(&Inbound {
        method: "POST",
        path: INBOX,
        query,
        headers: &headers,
        body,
    })
}

fn release(identity: &str) -> Vec<u8> {
    proved(&envelope(identity, &[DISCOVERY]), DOMAIN, &OLD)
        .to_string()
        .into_bytes()
}

fn pushed(world: &World, body: &[u8], nonce: &str) -> Answered {
    push(world, body, &signed(body, (&OLD, &OLD), Some(nonce)))
}

/// A nonce no other push of these tests carries.
fn nonce() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed).to_string()
}

fn cursor(world: &World) -> Option<Cursor> {
    pollster::block_on(world.store.inner.cursor(&SourceName::new("inbox")))
        .unwrap()
        .map(|kept| kept.value)
}

/// Whether the store keeps the nonce `nonce` of the peer `peer`.
fn kept_nonce(world: &World, peer: &str, nonce: &str) -> bool {
    pollster::block_on(world.store.inner.seen("inbox", &format!("{peer}/{nonce}"))).unwrap()
}

/// Whether the store keeps nothing of any request of `peer`: no window and
/// no audit, its own or the strangers'.
fn kept_nothing(world: &World, peer: &str) -> bool {
    let store = &world.store.inner;
    [peer, AuditDay::STRANGERS].into_iter().all(|owner| {
        pollster::block_on(store.window(owner)).unwrap().is_none()
            && pollster::block_on(store.audits(owner)).unwrap().is_empty()
    })
}

fn audit(world: &World, peer: &str) -> Vec<(u16, String)> {
    pollster::block_on(world.store.inner.audits(peer))
        .unwrap()
        .iter()
        .flat_map(|day: &AuditDay| &day.entries)
        .map(|entry| (entry.outcome, entry.reason.clone()))
        .collect()
}

#[test]
fn a_push_is_taken_its_proof_kept_and_its_event_left_to_announce() {
    let world = world();
    let body = release("github.com/1/v1");
    let answered = pushed(&world, &body, &nonce());
    assert_eq!(
        said(&answered),
        (202, TEXT, "herald took the envelope".to_owned())
    );
    assert_eq!(answered.refused, None);
    let kept = cursor(&world).unwrap();
    assert_eq!(
        kept.proved[0].identity.as_str(),
        "herald/upstream/github.com/1/v1"
    );
    assert_eq!(kept.proved[0].maker, "upstream");
    let Some(Job::Pushed {
        source,
        event: Some(event),
        withdrawn: None,
    }) = answered.job.clone()
    else {
        panic!("{:?}", answered.job);
    };
    assert_eq!(source.as_str(), "inbox");
    assert_eq!(event.identity().as_str(), "herald/upstream/github.com/1/v1");
    let Did::Announced(announced) = world.work(answered.job.unwrap()) else {
        panic!("not announced");
    };
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].route.as_str(), "mirror");
    assert_eq!(audit(&world, "upstream"), [(202, "taken".to_owned())]);
    let again = pushed(&world, &body, &nonce());
    assert_eq!(again.reply.status, 202);
    assert!(again.job.is_none());
    assert_eq!(
        audit(&world, "upstream")[1],
        (202, "no newer than the proof the source keeps".to_owned())
    );
}

#[test]
fn a_tombstone_pushed_leaves_the_identity_it_withdraws() {
    let world = world();
    pushed(&world, &release("github.com/1/v1"), &nonce());
    let tombstone = proved(
        &json!({ "type": "herald:Tombstone", "identity": "github.com/1/v1", "path": [DISCOVERY] }),
        DOMAIN,
        &OLD,
    );
    let later = crate::pulls::proved_at(&tombstone, (DOMAIN, &OLD), at());
    let answered = pushed(&world, later.to_string().as_bytes(), &nonce());
    let Some(Job::Pushed {
        event: None,
        withdrawn: Some((identity, project)),
        ..
    }) = answered.job.clone()
    else {
        panic!("{:?}", answered.job);
    };
    assert_eq!(identity.as_str(), "herald/upstream/github.com/1/v1");
    assert_eq!(project.as_deref(), Some("gate"));
    let Did::Announced(withdrawn) = world.work(answered.job.unwrap()) else {
        panic!("not withdrawn");
    };
    assert_eq!(withdrawn.len(), 1);
    assert_eq!(withdrawn[0].route.as_str(), "mirror");
}

#[test]
fn a_push_no_peer_signed_is_refused_and_kept_as_a_stranger_s() {
    let world = world();
    let body = release("github.com/1/v1");
    let unsigned = push(
        &world,
        &body,
        &[("host".to_owned(), "herald.example".to_owned())],
    );
    assert_eq!(
        said(&unsigned),
        (
            401,
            TEXT,
            "herald takes pushes signed by its peers only".to_owned()
        )
    );
    let stranger = push(
        &world,
        &body,
        &signed(&body, (&STRANGER, &STRANGER), Some(nonce().as_str())),
    );
    assert_eq!(stranger.reply.status, 401);
    assert_eq!(stranger.refused.as_deref(), Some("no peer holds the key"));
    let forged = push(
        &world,
        &body,
        &signed(&body, (&OLD, &STRANGER), Some(nonce().as_str())),
    );
    assert_eq!(
        forged.refused.as_deref(),
        Some("the signature does not hold")
    );
    let unnonced = push(&world, &body, &signed(&body, (&OLD, &OLD), None));
    assert_eq!(unnonced.reply.status, 401);
    let strangers = pollster::block_on(world.store.inner.audits(AuditDay::STRANGERS)).unwrap();
    let kept: Vec<_> = strangers.iter().flat_map(|day| &day.entries).collect();
    assert_eq!(kept.len(), 2);
    assert_eq!(
        kept[0].keyid.as_deref(),
        Some(method(DOMAIN, &STRANGER).as_str())
    );
    assert_eq!(kept[0].peer, None);
    assert_eq!(
        (kept[1].keyid.as_deref(), kept[1].reason.as_str()),
        (
            Some(method(DOMAIN, &OLD).as_str()),
            "the signature does not hold"
        )
    );
    assert_eq!(audit(&world, "upstream"), []);
    assert!(cursor(&world).is_none());
}

#[test]
fn a_nonce_is_taken_once() {
    let world = world();
    let once = nonce();
    assert_eq!(
        pushed(&world, &release("github.com/1/v1"), &once)
            .reply
            .status,
        202
    );
    let replayed = pushed(&world, &release("github.com/1/v2"), &once);
    assert_eq!(replayed.reply.status, 401);
    assert_eq!(
        replayed.refused.as_deref(),
        Some("the nonce was seen before")
    );
}

#[test]
fn a_push_larger_than_its_peer_may_send_is_refused_before_it_is_read() {
    let mut config = pushing("", "");
    let mut roomy = config.peers["upstream"].clone();
    roomy.grants.clear();
    roomy.url = "https://roomy.example".to_owned();
    roomy.domain = "roomy.example".to_owned();
    roomy.keys = vec![STRANGER];
    config.peers.insert("roomy".to_owned(), roomy);
    config.peers.get_mut("upstream").unwrap().most = 2048;
    let world = World::with(config, Script::default());
    let host = [("host".to_owned(), "herald.example".to_owned())];
    let largest = herald_config::Peer::MOST;
    let over = push(&world, &vec![b' '; largest + 1], &host);
    assert_eq!(
        said(&over),
        (413, TEXT, "the push is larger than herald reads".to_owned())
    );
    assert_eq!(push(&world, &vec![b' '; largest], &host).reply.status, 401);
    let fits = vec![b' '; 2048];
    let read = push(
        &world,
        &fits,
        &signed(&fits, (&OLD, &OLD), Some(nonce().as_str())),
    );
    assert_eq!(read.refused.as_deref(), Some("the body is no envelope"));
    let body = vec![b' '; 2049];
    let once = nonce();
    let small = push(&world, &body, &signed(&body, (&OLD, &OLD), Some(&once)));
    assert_eq!(small.reply.status, 413);
    assert_eq!(
        small.refused.as_deref(),
        Some("more than the 2048 bytes upstream, whose key it names, may send")
    );
    assert!(!kept_nonce(&world, "upstream", &once));
}

#[test]
fn a_push_larger_than_its_peer_may_send_keeps_nothing() {
    let mut config = pushing("", "");
    let mut roomy = config.peers["upstream"].clone();
    roomy.grants.clear();
    roomy.url = "https://roomy.example".to_owned();
    roomy.domain = "roomy.example".to_owned();
    roomy.keys = vec![STRANGER];
    config.peers.insert("roomy".to_owned(), roomy);
    config.peers.get_mut("upstream").unwrap().most = 2048;
    let world = World::with(config, Script::default());
    let host = [("host".to_owned(), "herald.example".to_owned())];
    let huge = vec![b' '; herald_config::Peer::MOST + 1];
    assert_eq!(push(&world, &huge, &host).reply.status, 413);
    let body = vec![b' '; 2049];
    let once = nonce();
    for _ in 0..3 {
        let refused = push(&world, &body, &signed(&body, (&OLD, &OLD), Some(&once)));
        assert_eq!(refused.reply.status, 413);
    }
    assert!(kept_nothing(&world, "upstream"));
    assert!(!kept_nonce(&world, "upstream", &once));
    assert!(cursor(&world).is_none());
}

#[test]
fn a_push_the_source_does_not_grant_or_take_is_forbidden() {
    let ended = World::with(
        pushing(
            ", until = \"2027-01-01T00:00:00Z\"",
            ", until = \"2027-01-01T00:00:00Z\"",
        ),
        Script::default(),
    );
    let answered = pushed(&ended, &release("github.com/1/v1"), &nonce());
    assert_eq!(
        said(&answered),
        (
            403,
            TEXT,
            "herald does not take this push from the peer".to_owned()
        )
    );
    assert_eq!(
        answered.refused.as_deref(),
        Some("the peer's grant to push to the source has ended")
    );
    let world = world();
    let body = b"not json".to_vec();
    let unread = push(
        &world,
        &body,
        &signed(&body, (&OLD, &OLD), Some(nonce().as_str())),
    );
    assert_eq!(unread.refused.as_deref(), Some("the body is no envelope"));
    let mut forged: Value = serde_json::from_slice(&release("github.com/1/v1")).unwrap();
    forged["title"] = json!("forged");
    let body = forged.to_string().into_bytes();
    let unproved = push(
        &world,
        &body,
        &signed(&body, (&OLD, &OLD), Some(nonce().as_str())),
    );
    assert_eq!(unproved.reply.status, 403);
    assert_eq!(
        unproved.refused.as_deref(),
        Some("an envelope's proof does not hold")
    );
}

#[test]
fn a_push_to_a_source_that_is_no_inbox_is_forbidden_and_off_the_domain_not_found() {
    let world = world();
    let body = release("github.com/1/v1");
    let headers = signed(&body, (&OLD, &OLD), Some(nonce().as_str()));
    let elsewhere: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str(),
                if name == "host" {
                    "other.example"
                } else {
                    value.as_str()
                },
            )
        })
        .collect();
    let off = world.answer(&Inbound {
        method: "POST",
        path: INBOX,
        query: None,
        headers: &elsewhere,
        body: &body,
    });
    assert_eq!(off.reply.status, 404);
    let other = signed_at(
        "https://herald.example/herald/v1/inbox/missing",
        &body,
        (&OLD, &OLD),
        Some(nonce().as_str()),
    );
    let named: Vec<(&str, &str)> = other
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let missing = world.answer(&Inbound {
        method: "POST",
        path: "/herald/v1/inbox/missing",
        query: None,
        headers: &named,
        body: &body,
    });
    assert_eq!(missing.reply.status, 403);
    assert_eq!(
        missing.refused.as_deref(),
        Some("the source grants the peer no push")
    );
    let got = world.answer(&Inbound {
        method: "GET",
        ..get(INBOX)
    });
    assert_eq!(got.reply.status, 405);
    assert_eq!(world.answer(&get("/herald/v1/inbox/%zz")).reply.status, 404);
}

#[test]
fn a_push_whose_digest_names_another_body_keeps_nothing() {
    let world = world();
    let body = release("github.com/1/v1");
    let headers = signed(&body, (&OLD, &OLD), Some(nonce().as_str()));
    let other = release("github.com/1/v2");
    let refused = push(&world, &other, &headers);
    assert_eq!(refused.reply.status, 401);
    assert_eq!(
        refused.refused.as_deref(),
        Some("the request carries no digest of its body")
    );
    assert!(kept_nothing(&world, "upstream"));
}

#[test]
fn a_push_is_checked_against_the_query_its_target_carries() {
    let world = world();
    let body = release("github.com/1/v1");
    let headers = signed_at(
        &format!("{URL}?via=relay"),
        &body,
        (&OLD, &OLD),
        Some(nonce().as_str()),
    );
    assert_eq!(push(&world, &body, &headers).reply.status, 401);
    assert_eq!(
        push_asking(&world, Some("via=relay"), &body, &headers)
            .reply
            .status,
        202
    );
}

#[test]
fn a_push_herald_cannot_keep_is_unavailable_and_its_audit_said() {
    for (fault, reason) in [
        (Fault::ReadsWindows, "broken"),
        (Fault::WritesWindows, "broken"),
        (Fault::ConflictsWindows, "the peer's window kept changing"),
        (Fault::SeesDeliveries, "broken"),
        (Fault::Admits, "broken"),
        (Fault::Cursors, "broken"),
        (Fault::WritesCursors, "broken"),
        (Fault::CursorConflicts, "the source's cursor kept changing"),
    ] {
        let mut world = world();
        world.store.fault = fault;
        let answered = pushed(&world, &release("github.com/1/v1"), &nonce());
        assert_eq!(
            said(&answered),
            (503, TEXT, "herald cannot take the push now".to_owned()),
            "{fault:?}"
        );
        assert!(
            answered.refused.as_deref().unwrap().contains(reason),
            "{answered:?}"
        );
        let counted = !matches!(
            fault,
            Fault::ReadsWindows
                | Fault::WritesWindows
                | Fault::ConflictsWindows
                | Fault::SeesDeliveries
        );
        assert_eq!(!audit(&world, "upstream").is_empty(), counted, "{fault:?}");
    }
    for fault in [
        Fault::WritesAudits,
        Fault::ConflictsAudits,
        Fault::ReadsAudits,
    ] {
        let mut world = world();
        world.store.fault = fault;
        let answered = pushed(&world, &release("github.com/1/v1"), &nonce());
        assert_eq!(answered.reply.status, 202);
        assert!(
            answered
                .refused
                .as_deref()
                .unwrap()
                .starts_with("the audit was not kept: "),
            "{answered:?}"
        );
        let body = release("github.com/1/v2");
        let refused = push(
            &world,
            &body,
            &signed(&body, (&STRANGER, &STRANGER), Some(nonce().as_str())),
        );
        assert!(
            refused
                .refused
                .as_deref()
                .unwrap()
                .contains("; the audit was not kept: ")
        );
    }
}

#[test]
fn doctor_names_each_grant_and_those_with_no_end_ended_or_ending_soon() {
    let notes = |until: &str| {
        let until = if until.is_empty() {
            String::new()
        } else {
            format!(", until = \"{until}\"")
        };
        herald_flows::granted(&pushing(&until, &until), at()).notes
    };
    assert_eq!(
        notes(""),
        [
            "peers.upstream may push sources.inbox with no end.",
            "peers.upstream: its grant on sources.inbox never ends; give it an until, so a key stolen from the peer pushes only for so long.",
            REFUSED_NOTE,
        ]
    );
    assert_eq!(
        notes("2027-01-15T08:00:00Z")[1],
        "peers.upstream: its grant on sources.inbox ended at 2027-01-15T08:00:00Z, so herald refuses what it pushes; give it a later until, or remove it."
    );
    assert_eq!(
        notes("2027-01-22T08:00:00Z")[1],
        "peers.upstream: its grant on sources.inbox ends at 2027-01-22T08:00:00Z; give it a later until to keep taking what it pushes."
    );
    assert_eq!(
        notes("2027-01-22T08:00:01Z"),
        [
            "peers.upstream may push sources.inbox until 2027-01-22T08:00:01Z.",
            REFUSED_NOTE
        ]
    );
}

#[test]
fn a_push_handed_on_to_be_worked_leaves_its_work_again_uncounted_and_unaudited() {
    let world = World::with(
        {
            let mut config = pushing("", "");
            config.peers.get_mut("upstream").unwrap().rate =
                herald_core::Rate::new(std::num::NonZeroU32::MIN, herald_core::Per::Minute);
            config
        },
        Script::default(),
    );
    let body = release("github.com/1/v1");
    let headers = signed(&body, (&OLD, &OLD), Some(nonce().as_str()));
    let borrowed: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let inbound = Inbound {
        method: "POST",
        path: INBOX,
        query: None,
        headers: &borrowed,
        body: &body,
    };
    assert_eq!(world.answer(&inbound).reply.status, 202);
    let again = world.answer_again(&inbound);
    assert_eq!(again.reply.status, 202);
    let Some(Job::Pushed {
        event: Some(event), ..
    }) = again.job
    else {
        panic!("{:?}", again.job);
    };
    assert_eq!(event.identity().as_str(), "herald/upstream/github.com/1/v1");
    assert_eq!(audit(&world, "upstream").len(), 1);
    assert_eq!(world.answer(&inbound).reply.status, 401);
}

#[test]
fn a_source_taking_only_pushes_is_never_polled() {
    let config = pushing("", "");
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let clock = FakeClock::at(at());
    let store = crate::harness::healthy();
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let swept = pollster::block_on(herald_flows::poll(
        &config,
        &connected,
        &ports,
        &FakeVerifier,
        "r1",
    ));
    assert!(
        swept.iter().all(|swept| swept.source.as_str() != "inbox"),
        "{swept:?}"
    );
}

#[test]
fn a_push_whose_peers_kept_keys_cannot_be_read_is_unavailable() {
    let signed_peer = |config: &mut Config, name: &str| {
        let peer = config.peers.get_mut(name).unwrap();
        peer.rotation = herald_config::Rotation::Signed;
    };
    let mut pusher = pushing("", "");
    signed_peer(&mut pusher, "upstream");
    let mut other = pushing("", "");
    let mut roomy = other.peers["upstream"].clone();
    roomy.grants.clear();
    roomy.url = "https://roomy.example".to_owned();
    roomy.domain = "roomy.example".to_owned();
    roomy.keys = vec![STRANGER];
    roomy.rotation = herald_config::Rotation::Signed;
    other.peers.insert("roomy".to_owned(), roomy);
    for config in [pusher, other] {
        let mut world = World::with(config, Script::default());
        world.store.fault = Fault::ReadsPeerKeys;
        let answered = pushed(&world, &release("github.com/1/v1"), &nonce());
        assert_eq!(answered.reply.status, 503, "{answered:?}");
        assert!(
            answered.refused.as_deref().unwrap().contains("broken"),
            "{answered:?}"
        );
    }
}

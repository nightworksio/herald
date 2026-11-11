use std::sync::Mutex;

use herald_core::{Addressed, Entry, Sent, Step, activity_claim, key, public_pem};
use herald_crypto::RingVerifier;
use herald_dest_fediverse::{ActivityPub, ActorRouteOptions};
use herald_ports::{Algorithm, Secret, Signer as _, Verifier as _};
use herald_testing::sample;

use crate::*;

const RSA: &str = include_str!("../../../herald-crypto/fixtures/rsa-2048-pkcs8.test.hex");

#[derive(Default)]
struct Kept(Mutex<Option<Request>>);

impl Http for Kept {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        *self.0.lock().unwrap() = Some(request);
        Ok(Response {
            status: 202,
            headers: Vec::new(),
            body: Vec::new(),
        })
    }
}

pub(super) fn pkcs8() -> String {
    use base64::Engine as _;
    let der: Vec<u8> = (0..RSA.trim().len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&RSA[index..index + 2], 16).unwrap())
        .collect();
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        base64::engine::general_purpose::STANDARD.encode(der)
    )
}

#[test]
fn a_delivery_herald_signs_is_one_its_own_inbox_takes_under_the_key_its_actor_publishes() {
    let secret = Secret::new("HERALD_ACTIVITYPUB_KEY", pkcs8());
    let actor = ActivityPub::new(
        "herald.example",
        "",
        secret.clone(),
        ActorRouteOptions {
            actor: "php-mutation-gate".to_owned(),
            content_warning: None,
            visibility: herald_dest_fediverse::Audience::Public,
            language: None,
        },
    );
    let now: jiff::Timestamp = "2026-11-03T08:00:00Z".parse().unwrap();
    let announcement = sample::announcement();
    let kept = Kept::default();
    let sent = pollster::block_on(actor.send(
        &kept,
        &RingSigner,
        now,
        &Step::Deliver("https://social.example:8443/inbox".to_owned()),
        &announcement,
        &Entry::new(announcement.key),
    ))
    .unwrap();
    assert!(matches!(sent, Sent::Landed(_)), "{sent:?}");
    let request = kept.0.lock().unwrap().take().unwrap();
    let headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(name, value)| (name.clone(), value.redacted()))
        .chain([("Host".to_owned(), "social.example:8443".to_owned())])
        .collect();
    let named: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let to = Addressed {
        target: "post /inbox",
        audience: "social.example:8443",
    };
    let claim = activity_claim(
        to,
        &named,
        &request.body,
        now,
        &herald_core::Inbound::DEFAULT,
    )
    .unwrap();
    assert_eq!(
        claim.signer.as_deref(),
        Some("https://herald.example/users/php-mutation-gate#main-key")
    );
    let published = pollster::block_on(RingSigner.public(Algorithm::RsaSha256, &secret)).unwrap();
    let public = key(herald_dest_fediverse::ACTOR_SCHEME, &public_pem(&published)).unwrap();
    let holds = |body: &[u8]| {
        claim.signatures.iter().any(|signature| {
            pollster::block_on(RingVerifier.verify(
                Algorithm::RsaSha256,
                &public,
                &claim.message,
                signature,
            ))
        }) && activity_claim(to, &named, body, now, &herald_core::Inbound::DEFAULT).is_ok()
    };
    assert!(holds(&request.body));
    assert!(!holds(b"{\"type\":\"Undo\"}"));
}

#[test]
fn a_public_base_connects_to_any_public_address_and_another_to_its_origin_alone() {
    assert!(native(herald_ports::PUBLIC, &herald_ports::Outbound::default()).is_ok());
    assert!(
        native(
            "https://discord.com/api/v10",
            &herald_ports::Outbound::default()
        )
        .is_ok()
    );
    assert!(native("http://discord.com", &herald_ports::Outbound::default()).is_err());
    assert!(
        native(
            "waiting+http://127.0.0.1:9001",
            &herald_ports::Outbound::default()
        )
        .is_err()
    );
    assert!(waiting("http://127.0.0.1:9001", &herald_ports::Outbound::default()).is_ok());
    assert!(waiting("http://discord.com", &herald_ports::Outbound::default()).is_err());
}

fn followed(key: Option<String>) -> super::world::Setup {
    let mut setup = super::world::setup(&format!(
        "{}{}",
        super::world::CONFIG,
        r#"[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
[[routes]]
id = "followed"
sources = ["cli"]
destination = "fedi"
template = "release"
actor = "cli"
"#
    ));
    if let Some(key) = key {
        setup.vars.insert("HERALD_ACTIVITYPUB_KEY".to_owned(), key);
    }
    setup
}

const UNKEYED: &str = "the destination \"fedi\" signs with HERALD_ACTIVITYPUB_KEY, which is not an RSA private key of 2048 to 8192 bits in a PKCS#8 PEM";

#[test]
fn doctor_loads_the_actor_s_key_where_it_is_set() {
    let world = super::world::World::default();
    let served = ["doctor", "--layer", "server"];
    let keyed = super::world::herald(&followed(Some(pkcs8())), &world, &served);
    assert_eq!(keyed.code, DONE, "{}", keyed.out);
    assert!(
        !keyed.out.contains("HERALD_ACTIVITYPUB_KEY"),
        "{}",
        keyed.out
    );
    let unset = super::world::herald(&followed(None), &world, &served);
    assert_eq!(unset.code, DONE, "{}", unset.out);
    assert!(
        unset
            .out
            .lines()
            .any(|line| line.starts_with("Nothing that reads ")
                && line.contains("HERALD_ACTIVITYPUB_KEY")),
        "{}",
        unset.out
    );
    let public = public_pem(
        &pollster::block_on(herald_crypto::RingSigner.public(
            Algorithm::RsaSha256,
            &Secret::new("HERALD_ACTIVITYPUB_KEY", pkcs8()),
        ))
        .unwrap(),
    );
    for bad in ["not a key".to_owned(), public] {
        let mistaken = super::world::herald(&followed(Some(bad)), &world, &served);
        assert_eq!(mistaken.code, MISTAKEN, "{}", mistaken.out);
        assert!(mistaken.out.contains(UNKEYED), "{}", mistaken.out);
    }
}

#[test]
fn a_run_whose_actor_key_cannot_sign_sends_nothing() {
    let world = super::world::World::default();
    let ran = super::world::herald(
        &followed(Some("not a key".to_owned())),
        &world,
        &super::world::ANNOUNCE,
    );
    assert_eq!(ran.code, STOPPED, "{}", ran.out);
    assert!(ran.err.contains(UNKEYED), "{}", ran.err);
}

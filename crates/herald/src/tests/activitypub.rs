use std::sync::Mutex;

use herald_core::{Addressed, Entry, Scheme, Sent, Step, activity_claim, key, public_pem};
use herald_dest_fediverse::{ActivityPub, ActorRouteOptions};
use herald_native::RingVerifier;
use herald_ports::{Algorithm, Secret, Signer as _, Verifier as _};
use herald_testing::sample;

use crate::*;

const RSA: &str = include_str!("../../../herald-native/fixtures/rsa-2048-pkcs8.test.hex");

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

fn pkcs8() -> String {
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
        secret.clone(),
        ActorRouteOptions {
            actor: "php-mutation-gate".to_owned(),
            content_warning: None,
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
    let claim = activity_claim(to, &named, &request.body, now).unwrap();
    assert_eq!(
        claim.signer.as_deref(),
        Some("https://herald.example/users/php-mutation-gate#main-key")
    );
    let published = pollster::block_on(RingSigner.public(Algorithm::RsaSha256, &secret)).unwrap();
    let public = key(Scheme::ActivityPub, &public_pem(&published)).unwrap();
    let holds = |body: &[u8]| {
        claim.signatures.iter().any(|signature| {
            pollster::block_on(RingVerifier.verify(
                Algorithm::RsaSha256,
                &public,
                &claim.message,
                signature,
            ))
        }) && activity_claim(to, &named, body, now).is_ok()
    };
    assert!(holds(&request.body));
    assert!(!holds(b"{\"type\":\"Undo\"}"));
}

#[test]
fn a_public_base_connects_to_any_public_address_and_another_to_its_origin_alone() {
    assert!(native(herald_ports::PUBLIC).is_ok());
    assert!(native("https://discord.com/api/v10").is_ok());
    assert!(native("http://discord.com").is_err());
}

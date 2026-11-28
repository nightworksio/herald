//! A Teams bot's endpoint with herald's own signer and verifier, against an
//! RSA key made at run time: an activity is taken on a token that key
//! signs, and refused on one another key signs under its id or on one
//! signed by a key the Bot Framework does not endorse for Teams.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_config::{Uses, load};
use herald_core::der::{BIT_STRING, INTEGER, SEQUENCE, element};
use herald_crypto::keygen::{Bits, rsa_key};
use herald_crypto::{RingSigner, RingVerifier};
use herald_flows::{Admission, Answered, Budget, Inbound, Keys, Ports, Serving, Webhooks};
use herald_ports::{Algorithm, Http, HttpError, Request, Response, Secret, Signer as _};
use herald_records::MemoryKeyring;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeStore};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

const APP: &str = "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const METADATA: &str = "https://login.example/v1/.well-known/openidconfiguration";
const KEYS: &str = "https://login.example/v1/.well-known/keys";
const ISSUER: &str = "https://api.example";
const SERVICE: &str = "https://smba.example/emea/";

/// The Bot Framework's two documents, answered by their address.
#[derive(Clone)]
struct Published(BTreeMap<String, String>);

impl Http for Published {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let body = self.0.get(&request.url.redacted()).cloned();
        Ok(Response {
            status: if body.is_some() { 200 } else { 404 },
            headers: Vec::new(),
            body: body.unwrap_or_default().into_bytes(),
        })
    }
}

/// An RSA key made at run time: its private half, and its modulus and
/// exponent as a key set lists them.
struct Pair {
    private: Secret,
    listed: (String, String),
}

fn pair() -> Pair {
    let made = rsa_key(Bits::Rsa2048).unwrap();
    let (spki, _) = element(&made.public, SEQUENCE).unwrap();
    let (_, rest) = element(spki, SEQUENCE).unwrap();
    let (bits, _) = element(rest, BIT_STRING).unwrap();
    let (public, _) = element(&bits[1..], SEQUENCE).unwrap();
    let (modulus, rest) = element(public, INTEGER).unwrap();
    let (exponent, _) = element(rest, INTEGER).unwrap();
    Pair {
        private: Secret::new("the test's RSA key", made.private.as_str()),
        listed: (
            URL_SAFE_NO_PAD.encode(modulus),
            URL_SAFE_NO_PAD.encode(exponent),
        ),
    }
}

fn token(kid: &str, pair: &Pair, now: Timestamp) -> String {
    let part = |value: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).unwrap());
    let signed = format!(
        "{}.{}",
        part(&json!({ "typ": "JWT", "alg": "RS256", "kid": kid })),
        part(&json!({
            "iss": ISSUER,
            "aud": APP,
            "nbf": now.as_second() - 60,
            "exp": now.as_second() + 3600,
            "serviceUrl": SERVICE,
        }))
    );
    let signature = pollster::block_on(RingSigner::default().sign(
        Algorithm::RsaSha256,
        &pair.private,
        signed.as_bytes(),
    ))
    .unwrap();
    format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn answered(published: &Published, token: &str, now: Timestamp) -> Answered {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.teams]
kind = "teams"
app_id = "{APP}"
app_password = {{ secret = "HERALD_TEAMS_APP_PASSWORD" }}
service = "https://smba.example/teams/"
openid = "{METADATA}"
issuer = "{ISSUER}"
[destinations.teams.interactions]
"#
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &herald_compose::kinds(),
    )
    .unwrap();
    let secrets = FakeSecrets::default().with("HERALD_TEAMS_APP_PASSWORD", "app-secret");
    let connected = pollster::block_on(herald_flows::prepare(
        &config,
        &secrets,
        &FakeSigner,
        Uses::Serves,
    ))
    .unwrap()
    .connect(|_| Ok::<_, ()>(published.clone()))
    .unwrap();
    let store = FakeStore::default();
    let clock = FakeClock::at(now);
    let keyring = MemoryKeyring::new(8);
    let authorization = format!("Bearer {token}");
    let body = json!({
        "type": "message",
        "id": "1481567603816",
        "serviceUrl": SERVICE,
        "channelId": "msteams",
        "conversation": { "id": "19:abc@thread.tacv2" },
    })
    .to_string();
    pollster::block_on(herald_flows::answer(
        &Serving {
            config: &config,
            connected: &connected,
            ports: &Ports {
                store: &store,
                clock: &clock,
                signer: &FakeSigner,
                budget: Budget::UNBOUNDED,
            },
            verifier: &RingVerifier,
            public: published,
            keys: &Keys {
                keyring: &keyring,
                refreshed: SignedDuration::from_hours(1),
                reread: SignedDuration::from_mins(1),
            },
            webhooks: &Webhooks::default(),
            admission: Admission::Once,
            merging: None,
        },
        &Inbound {
            method: "POST",
            path: "/interactions/teams",
            query: None,
            headers: &[("Authorization", &authorization)],
            body: body.as_bytes(),
        },
    ))
}

#[test]
fn an_activity_is_taken_on_a_token_a_key_endorsed_for_teams_signs_and_refused_on_any_other() {
    let (teams, speech, stranger) = (pair(), pair(), pair());
    let listed = |kid: &str, pair: &Pair, endorsed: &str| {
        json!({
            "kty": "RSA",
            "use": "sig",
            "kid": kid,
            "n": pair.listed.0,
            "e": pair.listed.1,
            "endorsements": [endorsed],
        })
    };
    let published = Published(BTreeMap::from([
        (
            METADATA.to_owned(),
            json!({
                "issuer": ISSUER,
                "jwks_uri": KEYS,
                "id_token_signing_alg_values_supported": ["RS256"],
            })
            .to_string(),
        ),
        (
            KEYS.to_owned(),
            json!({ "keys": [listed("k1", &teams, "msteams"), listed("k2", &speech, "telephony")] })
                .to_string(),
        ),
    ]));
    let now: Timestamp = "2027-01-15T08:00:00Z".parse().unwrap();
    let status = |token: &str| {
        let answered = answered(&published, token, now);
        let why = answered.refused.clone().unwrap_or_default();
        assert!(!why.contains(token), "{why}");
        (answered.reply.status, why)
    };
    assert_eq!(status(&token("k1", &teams, now)), (200, String::new()));
    assert_eq!(
        status(&token("k1", &stranger, now)),
        (
            401,
            "the request's signature is not its sender's".to_owned()
        )
    );
    assert_eq!(status(&token("k2", &speech, now)).0, 403);
}

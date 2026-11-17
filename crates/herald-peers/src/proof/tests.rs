use serde_json::json;
use sha2::Sha256;

use super::*;

const KEY_ID: &str = "https://herald.example/.well-known/herald#z6Mkkey";

const SIGNATURE: [u8; SIGNATURE_LENGTH] = [7; SIGNATURE_LENGTH];

fn written() -> String {
    multibase::encode(&SIGNATURE)
}

fn at() -> Timestamp {
    "2023-02-24T23:36:38.250Z".parse().unwrap_or_default()
}

fn event() -> Value {
    json!({
        "@context": ["https://www.w3.org/ns/credentials/v2", "https://herald.example/ns"],
        "identity": "github.com/nightworksio/herald/v1.0.0",
        "version": "1.0.0",
    })
}

#[test]
fn the_message_is_the_hash_of_the_proof_options_with_the_object_s_context_then_of_the_object() {
    let ready = unproved(&event(), KEY_ID, at());
    let options = json!({
        "@context": ["https://www.w3.org/ns/credentials/v2", "https://herald.example/ns"],
        "type": "DataIntegrityProof",
        "cryptosuite": "eddsa-jcs-2022",
        "created": "2023-02-24T23:36:38Z",
        "verificationMethod": KEY_ID,
        "proofPurpose": "assertionMethod",
    });
    let expected = [
        Sha256::digest(canonical(&options).unwrap_or_default()),
        Sha256::digest(canonical(&event()).unwrap_or_default()),
    ]
    .concat();
    assert_eq!(ready.map(|ready| ready.message().to_vec()), Ok(expected));
}

#[test]
fn a_proved_object_carries_the_proof_without_a_context_and_its_signature_in_base58btc() {
    let proved = unproved(&event(), KEY_ID, at()).map(|ready| ready.proved(&SIGNATURE));
    let mut expected = event();
    if let Some(members) = expected.as_object_mut() {
        members.insert(
            "proof".to_owned(),
            json!({
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "created": "2023-02-24T23:36:38Z",
                "verificationMethod": KEY_ID,
                "proofPurpose": "assertionMethod",
                "proofValue": written(),
            }),
        );
    }
    assert_eq!(proved, Ok(expected));
}

#[test]
fn a_shown_proof_names_its_key_and_the_same_message_it_was_made_over() {
    for document in [event(), json!({ "identity": "x", "n": 1.5 })] {
        let ready = unproved(&document, KEY_ID, at()).unwrap();
        let message = ready.message().to_vec();
        let proved = ready.proved(&SIGNATURE);
        assert_eq!(
            shown(&proved),
            Ok(Shown {
                method: KEY_ID.to_owned(),
                created: "2023-02-24T23:36:38Z".to_owned(),
                signature: SIGNATURE.to_vec(),
                message,
            })
        );
    }
}

#[test]
fn proving_again_replaces_the_proof_the_object_carried() {
    let first = unproved(&event(), KEY_ID, at()).unwrap();
    let message = first.message().to_vec();
    let proved = first.proved(&SIGNATURE);
    assert_eq!(
        unproved(&proved, KEY_ID, at()).map(|again| again.message().to_vec()),
        Ok(message)
    );
}

#[test]
fn a_proof_naming_the_start_of_the_object_s_context_is_checked_over_that_start() {
    let mut document = event();
    let mut start_only = event();
    if let Some(members) = start_only.as_object_mut() {
        members.insert(
            CONTEXT.to_owned(),
            json!(["https://www.w3.org/ns/credentials/v2"]),
        );
    }
    let ready = unproved(&start_only, KEY_ID, at()).unwrap();
    let message = ready.message().to_vec();
    if let (Some(members), Value::Object(proved)) =
        (document.as_object_mut(), ready.proved(&SIGNATURE))
    {
        let mut proof = proved.get(PROOF).cloned().unwrap_or_default();
        if let Some(fields) = proof.as_object_mut() {
            fields.insert(
                CONTEXT.to_owned(),
                json!(["https://www.w3.org/ns/credentials/v2"]),
            );
        }
        members.insert(PROOF.to_owned(), proof);
    }
    assert_eq!(shown(&document).map(|shown| shown.message), Ok(message));
}

#[test]
fn a_proof_whose_context_the_object_does_not_start_with_is_refused() {
    for held in [Some(json!(["https://other.example"])), None] {
        let mut document = json!({ "identity": "x" });
        if let Some(members) = document.as_object_mut() {
            if let Some(context) = held {
                members.insert(CONTEXT.to_owned(), context);
            }
            members.insert(
                PROOF.to_owned(),
                json!({
                    "@context": "https://www.w3.org/ns/credentials/v2",
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-jcs-2022",
                    "created": "2023-02-24T23:36:38Z",
                    "verificationMethod": KEY_ID,
                    "proofPurpose": "assertionMethod",
                    "proofValue": written(),
                }),
            );
        }
        assert_eq!(shown(&document), Err(Unproven::Context));
    }
}

fn with_proof(proof: &Value) -> Value {
    json!({ "identity": "x", "proof": proof })
}

fn good_proof() -> Map<String, Value> {
    json!({
        "type": "DataIntegrityProof",
        "cryptosuite": "eddsa-jcs-2022",
        "created": "2023-02-24T23:36:38Z",
        "verificationMethod": KEY_ID,
        "proofPurpose": "assertionMethod",
        "proofValue": written(),
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

#[test]
fn a_proof_of_another_shape_is_refused_by_what_is_wrong() {
    let changed = |name: &str, value: Option<Value>| {
        let mut proof = good_proof();
        match value {
            Some(value) => proof.insert(name.to_owned(), value),
            None => proof.remove(name),
        };
        with_proof(&Value::Object(proof))
    };
    for (document, refused) in [
        (json!([1]), Unproven::NotAnObject),
        (json!({ "identity": "x" }), Unproven::Unproved),
        (
            with_proof(&json!([good_proof()])),
            Unproven::Shape("it is not one object"),
        ),
        (
            changed("type", Some(json!("Ed25519Signature2020"))),
            Unproven::Shape("its type is not DataIntegrityProof"),
        ),
        (
            changed("cryptosuite", Some(json!("eddsa-rdfc-2022"))),
            Unproven::Shape("its cryptosuite is another one"),
        ),
        (
            changed("cryptosuite", None),
            Unproven::Shape("its cryptosuite is another one"),
        ),
        (
            changed("proofPurpose", Some(json!("authentication"))),
            Unproven::Shape("its purpose is not assertionMethod"),
        ),
        (
            changed("proofValue", None),
            Unproven::Shape("it holds no proofValue"),
        ),
        (
            changed("proofValue", Some(json!(7))),
            Unproven::Shape("it holds no proofValue"),
        ),
        (
            changed("proofValue", Some(json!("uSGVsbG8"))),
            Unproven::Shape("its proofValue is not an Ed25519 signature in base58btc"),
        ),
        (
            changed("proofValue", Some(json!(multibase::encode(&[7; 63])))),
            Unproven::Shape("its proofValue is not an Ed25519 signature in base58btc"),
        ),
        (
            changed("proofValue", Some(json!(multibase::encode(&[7; 65])))),
            Unproven::Shape("its proofValue is not an Ed25519 signature in base58btc"),
        ),
        (
            changed("verificationMethod", None),
            Unproven::Shape("it names no verification method"),
        ),
        (
            changed("created", Some(json!(1))),
            Unproven::Shape("it names no creation time"),
        ),
        (
            json!({ "n": 9_007_199_254_740_993_u64, "proof": good_proof() }),
            Unproven::Uncanonical(Uncanonical::Inexact {
                number: "9007199254740993".to_owned(),
            }),
        ),
    ] {
        assert_eq!(shown(&document), Err(refused));
    }
}

#[test]
fn an_object_that_cannot_be_proved_says_why() {
    assert_eq!(
        unproved(&json!("text"), KEY_ID, at()),
        Err(Unproven::NotAnObject)
    );
    assert_eq!(
        unproved(&json!({ "n": u64::MAX }), KEY_ID, at()),
        Err(Unproven::Uncanonical(Uncanonical::Inexact {
            number: u64::MAX.to_string()
        }))
    );
}

#[test]
fn every_refusal_reads_as_a_sentence() {
    for (refused, text) in [
        (Unproven::NotAnObject, "only a JSON object carries a proof"),
        (Unproven::Unproved, "the object carries no proof"),
        (
            Unproven::Shape("its purpose is not assertionMethod"),
            "the proof is not an eddsa-jcs-2022 proof: its purpose is not assertionMethod",
        ),
        (
            Unproven::Context,
            "the proof's @context is not the start of the object's",
        ),
        (
            Unproven::Uncanonical(Uncanonical::Inexact {
                number: "1".to_owned(),
            }),
            "the integer 1 reads as a double written otherwise, so it has no canonical JSON form; write it as a string",
        ),
    ] {
        assert_eq!(refused.to_string(), text);
    }
}

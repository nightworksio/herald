//! Integrity proofs on herald's objects: W3C Data Integrity proofs by the
//! `eddsa-jcs-2022` cryptosuite, made by the origin's Ed25519 key and kept
//! unchanged by whoever passes the object on.

use std::fmt;

use jiff::Timestamp;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::jcs::{Uncanonical, canonical};
use crate::multibase;

/// The member a proof travels in.
pub const PROOF: &str = "proof";
/// The member holding a JSON-LD context.
const CONTEXT: &str = "@context";
/// The proof's type.
const PROOF_TYPE: &str = "DataIntegrityProof";
/// The cryptosuite herald proves with.
const CRYPTOSUITE: &str = "eddsa-jcs-2022";
/// What herald's proofs assert.
const PURPOSE: &str = "assertionMethod";
/// The member holding the signature.
const PROOF_VALUE: &str = "proofValue";
/// The member naming a proof's type.
const TYPE: &str = "type";
/// The member naming a proof's cryptosuite.
const SUITE: &str = "cryptosuite";
/// The member naming when a proof was made.
const MADE: &str = "created";
/// The member naming the signing key.
const METHOD: &str = "verificationMethod";
/// The member naming what a proof asserts.
const PROOF_PURPOSE: &str = "proofPurpose";
/// The bytes of an Ed25519 signature.
pub const SIGNATURE_LENGTH: usize = 64;
/// How a proof's creation time is written: an XML Schema `dateTime` in UTC.
const CREATED_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// Why an object cannot be proved, or its proof cannot be checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unproven {
    /// The object is not a JSON object.
    NotAnObject,
    /// The object carries no proof.
    Unproved,
    /// The proof is not one object of herald's cryptosuite: what is wrong.
    Shape(&'static str),
    /// The proof's context is not the start of the object's.
    Context,
    /// The object or proof has no canonical form.
    Uncanonical(Uncanonical),
}

impl fmt::Display for Unproven {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => write!(f, "only a JSON object carries a proof"),
            Self::Unproved => write!(f, "the object carries no proof"),
            Self::Shape(what) => write!(f, "the proof is not an {CRYPTOSUITE} proof: {what}"),
            Self::Context => write!(f, "the proof's @context is not the start of the object's"),
            Self::Uncanonical(why) => why.fmt(f),
        }
    }
}

/// An object ready to be proved: the bytes the key signs, and the proof
/// the signature completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unproved {
    document: Map<String, Value>,
    proof: Map<String, Value>,
    message: Vec<u8>,
}

impl Unproved {
    /// The bytes the key signs: the SHA-256 of the canonical proof
    /// options, then the SHA-256 of the canonical object.
    #[must_use]
    pub fn message(&self) -> &[u8] {
        &self.message
    }

    /// The object carrying its proof, completed by `signature`.
    #[must_use]
    pub fn proved(self, signature: &[u8]) -> Value {
        let Self {
            mut document,
            mut proof,
            ..
        } = self;
        proof.remove(CONTEXT);
        proof.insert(
            PROOF_VALUE.to_owned(),
            Value::String(multibase::encode(signature)),
        );
        document.insert(PROOF.to_owned(), Value::Object(proof));
        Value::Object(document)
    }
}

/// `document`, without any proof it carries, ready to be proved by the
/// key `method` names, as made at `created`.
///
/// # Errors
///
/// The document is not an object, or has no canonical form.
pub fn unproved(document: &Value, method: &str, created: Timestamp) -> Result<Unproved, Unproven> {
    let mut document = document.as_object().ok_or(Unproven::NotAnObject)?.clone();
    document.remove(PROOF);
    let mut proof = Map::new();
    proof.insert(TYPE.to_owned(), Value::String(PROOF_TYPE.to_owned()));
    proof.insert(SUITE.to_owned(), Value::String(CRYPTOSUITE.to_owned()));
    proof.insert(
        MADE.to_owned(),
        Value::String(created.strftime(CREATED_FORMAT).to_string()),
    );
    proof.insert(METHOD.to_owned(), Value::String(method.to_owned()));
    proof.insert(PROOF_PURPOSE.to_owned(), Value::String(PURPOSE.to_owned()));
    if let Some(context) = document.get(CONTEXT) {
        proof.insert(CONTEXT.to_owned(), context.clone());
    }
    let message = hashed(&proof, &document)?;
    Ok(Unproved {
        document,
        proof,
        message,
    })
}

fn hashed(proof: &Map<String, Value>, document: &Map<String, Value>) -> Result<Vec<u8>, Unproven> {
    let options = canonical(&Value::Object(proof.clone())).map_err(Unproven::Uncanonical)?;
    let object = canonical(&Value::Object(document.clone())).map_err(Unproven::Uncanonical)?;
    Ok([Sha256::digest(options), Sha256::digest(object)].concat())
}

/// A proof shown on an object: the key it names, when it was made, its
/// signature, and the bytes that must be what the key signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// The verification method the proof names: the signing key's id.
    pub method: String,
    /// When the proof was made, as written.
    pub created: String,
    /// The signature.
    pub signature: Vec<u8>,
    /// The bytes the key must have signed.
    pub message: Vec<u8>,
}

/// The proof `document` carries, with the bytes its signature must be of.
///
/// # Errors
///
/// The document is not an object, carries no proof, its proof is not one
/// object of herald's cryptosuite, its contexts disagree, or it has no
/// canonical form.
pub fn shown(document: &Value) -> Result<Shown, Unproven> {
    let mut document = document.as_object().ok_or(Unproven::NotAnObject)?.clone();
    let proof = document.remove(PROOF).ok_or(Unproven::Unproved)?;
    let Value::Object(mut proof) = proof else {
        return Err(Unproven::Shape("it is not one object"));
    };
    let text = |proof: &Map<String, Value>, name: &str| {
        proof.get(name).and_then(Value::as_str).map(str::to_owned)
    };
    for (name, expected, wrong) in [
        (TYPE, PROOF_TYPE, "its type is not DataIntegrityProof"),
        (SUITE, CRYPTOSUITE, "its cryptosuite is another one"),
        (PROOF_PURPOSE, PURPOSE, "its purpose is not assertionMethod"),
    ] {
        if text(&proof, name).as_deref() != Some(expected) {
            return Err(Unproven::Shape(wrong));
        }
    }
    let written = proof
        .remove(PROOF_VALUE)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(Unproven::Shape("it holds no proofValue"))?;
    let signature = multibase::decode(&written, SIGNATURE_LENGTH)
        .filter(|signature| signature.len() == SIGNATURE_LENGTH)
        .ok_or(Unproven::Shape(
            "its proofValue is not an Ed25519 signature in base58btc",
        ))?;
    let method = text(&proof, METHOD).ok_or(Unproven::Shape("it names no verification method"))?;
    let created = text(&proof, MADE).ok_or(Unproven::Shape("it names no creation time"))?;
    if let Some(context) = proof.get(CONTEXT) {
        if !starts(document.get(CONTEXT), context) {
            return Err(Unproven::Context);
        }
        document.insert(CONTEXT.to_owned(), context.clone());
    }
    if let Some(context) = document.get(CONTEXT) {
        proof.insert(CONTEXT.to_owned(), context.clone());
    }
    let message = hashed(&proof, &document)?;
    Ok(Shown {
        method,
        created,
        signature,
        message,
    })
}

/// Whether the object's context `held` starts with every value of the
/// proof's context `context`, in order.
fn starts(held: Option<&Value>, context: &Value) -> bool {
    let listed = |value: &Value| match value {
        Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    };
    let Some(held) = held else {
        return false;
    };
    listed(held).starts_with(&listed(context))
}

#[cfg(test)]
mod tests;

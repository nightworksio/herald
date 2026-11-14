//! What an inbound request claims about who signed it, read from its raw
//! headers and body before anything parses the body (ADR-0007).
//!
//! The claim names the bytes that were signed and the signatures over them.
//! Checking a signature is left to a `Verifier`, so this step holds no key;
//! [`key`] reads the key a scheme checks with from the way it is written.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};

use crate::activity::RSA_SHA256;
use crate::bounds::Inbound;
use crate::der::{BIT_STRING, PUBLIC_KEY, RSA_ENCRYPTION, SEQUENCE, element, pem};
use crate::key::unhex;

/// Who signs a kind of inbound request, and how: data each source and
/// destination supplies, over the primitives herald checks (ADR-0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    /// Its name, as a store keeps its deliveries under it.
    pub name: &'static str,
    /// How a request of it is signed.
    pub signed: Signed,
    /// How the key it is checked with is written.
    pub key: KeyForm,
}

/// How a request of a scheme is signed: one of the primitives herald
/// checks, with the headers it is carried in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signed {
    /// An HMAC-SHA256 of the body, in hex after `prefix`, in the first of
    /// `headers` the request carries; delivered as its signature.
    Body {
        /// The headers it may be carried in, the first named when none is.
        headers: &'static [&'static str],
        /// What the hex follows.
        prefix: &'static str,
    },
    /// An HMAC-SHA256 of `<version>:<timestamp>:<body>`, in hex after
    /// `prefix` in `signature`, its Unix-seconds timestamp in `timestamp`;
    /// delivered as its signature.
    Stamped {
        /// The header the signature is in.
        signature: &'static str,
        /// What the hex follows.
        prefix: &'static str,
        /// The header the timestamp is in.
        timestamp: &'static str,
        /// The version the signed message starts with.
        version: &'static str,
    },
    /// Ed25519, in hex in `signature`, over the Unix-seconds timestamp in
    /// `timestamp` and the body; delivered as its signature.
    Ed25519 {
        /// The header the signature is in.
        signature: &'static str,
        /// The header the timestamp is in.
        timestamp: &'static str,
    },
    /// As Standard Webhooks sign: `v1,<base64>` HMAC-SHA256s in
    /// `signature`, each of `<id>.<timestamp>.<body>`; delivered as its id.
    StandardWebhooks {
        /// The header the id is in.
        id: &'static str,
        /// The header the timestamp is in.
        timestamp: &'static str,
        /// The header the signatures are in.
        signature: &'static str,
    },
    /// A bearer token in `header`, checked against the one herald holds.
    Bearer {
        /// The header the token is in.
        header: &'static str,
    },
    /// HTTP Signatures over where the request was sent, claimed by
    /// [`activity_claim`] alone.
    HttpSignature,
    /// A bearer token checked against the hashes herald keeps, never by a
    /// claim.
    Hashed,
}

/// How the key a scheme is checked with is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyForm {
    /// As its bytes.
    Bytes,
    /// In hex.
    Hex,
    /// In base64 after `whsec_`.
    Whsec,
    /// As a PEM public key, of RSA.
    RsaPem,
}

/// The kind of signature a scheme carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signature {
    /// HMAC over SHA-256, keyed by a shared secret.
    HmacSha256,
    /// Ed25519, checked with the sender's public key.
    Ed25519,
    /// A token the sender presents, checked against the one herald holds.
    Token,
    /// RSASSA-PKCS1-v1_5 over SHA-256, checked with the signer's public key.
    RsaSha256,
}

/// What a request claims: that one of `signatures` signs `message` under
/// `kind`. A sender rotating its key signs with each key it holds.
#[derive(Clone, PartialEq, Eq)]
pub struct Claim {
    /// The kind of signature.
    pub kind: Signature,
    /// The bytes that were signed.
    pub message: Vec<u8>,
    /// The signatures over them, any one of which holds.
    pub signatures: Vec<Vec<u8>>,
    /// What makes this delivery that delivery, as its signature covers it,
    /// which the store admits once (ADR-0013): the signed `webhook-id` of a
    /// GitLab webhook, and the signature itself, in hex, of every other
    /// signed request: a GitHub or Forgejo webhook, whose delivery ids no
    /// signature covers, a Discord interaction, a Slack request and an
    /// ActivityPub activity.
    pub delivery: Option<String>,
    /// The key it names its signer by, where its scheme fetches the key it
    /// is checked with: an ActivityPub actor's `keyId`.
    pub signer: Option<String>,
}

impl fmt::Debug for Claim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Claim")
            .field("kind", &self.kind)
            .field("message", &format_args!("{} bytes", self.message.len()))
            .field(
                "signatures",
                &format_args!("{} held back", self.signatures.len()),
            )
            .field("delivery", &self.delivery)
            .field("signer", &self.signer)
            .finish()
    }
}

/// Why a request is refused before its signature is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A header the scheme signs with is missing.
    Unsigned {
        /// The header.
        header: &'static str,
    },
    /// A header the scheme signs with is not in its form.
    Malformed {
        /// The header.
        header: &'static str,
    },
    /// The signed timestamp lies outside the replay window, this wide
    /// either way.
    Stale {
        /// The replay window.
        window: SignedDuration,
    },
    /// A header the scheme must sign is not among those signed.
    Uncovered {
        /// The header.
        header: &'static str,
    },
    /// A header the scheme checks does not match the body it was sent with.
    Unmatched {
        /// The header.
        header: &'static str,
    },
    /// It was signed for another host than this herald.
    Misdirected,
    /// An ActivityPub activity was claimed without where it was sent.
    Unaddressed,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsigned { header } => write!(f, "the request carries no {header}"),
            Self::Malformed { header } => write!(f, "the request's {header} is not in its form"),
            Self::Stale { window } => write!(
                f,
                "the request was signed more than {} minutes from now, so it is taken for a replay; check that the sender's clock and herald's are set right",
                window.as_mins()
            ),
            Self::Uncovered { header } => {
                write!(f, "the request's signature does not cover its {header}")
            }
            Self::Unmatched { header } => {
                write!(f, "the request's {header} does not match its body")
            }
            Self::Misdirected => f.write_str("the request was signed for another host"),
            Self::Unaddressed => f.write_str(
                "an ActivityPub activity is claimed with where it was sent, which no header says",
            ),
        }
    }
}

/// The claim a request of `scheme` makes, from its `headers` and raw `body`,
/// with `now` and `bounds` bounding any signed timestamp and how many
/// signatures it carries. Header names match in any case.
///
/// # Errors
///
/// The refusal, where a header is missing or malformed, or the signed
/// timestamp lies outside the replay window.
pub fn claim(
    scheme: Scheme,
    headers: &[(&str, &str)],
    body: &[u8],
    now: Timestamp,
    bounds: &Inbound,
) -> Result<Claim, Refusal> {
    match scheme.signed {
        Signed::Body {
            headers: names,
            prefix,
        } => {
            let (header, value) = names
                .iter()
                .find_map(|name| header_of(headers, name).map(|value| (*name, value)))
                .ok_or(Refusal::Unsigned {
                    header: names.first().copied().unwrap_or_default(),
                })?;
            let signature = value
                .strip_prefix(prefix)
                .and_then(unhex)
                .ok_or(Refusal::Malformed { header })?;
            Ok(signed_body(body, signature))
        }
        Signed::Ed25519 {
            signature,
            timestamp: stamped,
        } => {
            let stamp = timestamp(headers, stamped, now, bounds.replay_window)?;
            let signed = unhex(required(headers, signature)?)
                .ok_or(Refusal::Malformed { header: signature })?;
            Ok(Claim {
                kind: Signature::Ed25519,
                message: [stamp.as_bytes(), body].concat(),
                delivery: Some(crate::hex(&signed)),
                signatures: vec![signed],
                signer: None,
            })
        }
        Signed::Stamped {
            signature,
            prefix,
            timestamp: stamped,
            version,
        } => {
            let stamp = timestamp(headers, stamped, now, bounds.replay_window)?;
            let signed = prefixed(headers, signature, prefix)?;
            let delivery = crate::hex(&signed);
            Ok(hmac(
                [version.as_bytes(), b":", stamp.as_bytes(), b":", body].concat(),
                signed,
                Some(&delivery),
            ))
        }
        Signed::HttpSignature | Signed::Hashed => Err(Refusal::Unaddressed),
        Signed::Bearer { header } => {
            let token = bearer(required(headers, header)?).ok_or(Refusal::Malformed { header })?;
            Ok(Claim {
                kind: Signature::Token,
                message: Vec::new(),
                signatures: vec![token.as_bytes().to_vec()],
                delivery: None,
                signer: None,
            })
        }
        Signed::StandardWebhooks {
            id,
            timestamp: stamped,
            signature,
        } => {
            let id = required(headers, id)?;
            let stamp = timestamp(headers, stamped, now, bounds.replay_window)?;
            let signatures: Vec<Vec<u8>> = required(headers, signature)?
                .split(' ')
                .filter_map(|entry| entry.strip_prefix("v1,"))
                .map(|encoded| {
                    STANDARD
                        .decode(encoded)
                        .ok()
                        .filter(|signature| !signature.is_empty())
                })
                .take(bounds.signatures.saturating_add(1))
                .collect::<Option<_>>()
                .filter(|signatures: &Vec<Vec<u8>>| {
                    (1..=bounds.signatures).contains(&signatures.len())
                })
                .ok_or(Refusal::Malformed { header: signature })?;
            Ok(Claim {
                kind: Signature::HmacSha256,
                message: [id.as_bytes(), b".", stamp.as_bytes(), b".", body].concat(),
                signatures,
                delivery: Some(id.to_owned()),
                signer: None,
            })
        }
    }
}

/// The key a request of `scheme` is checked with, from the way it is
/// written: a GitLab signing token's base64 after its `whsec_`, a Discord
/// application's public key in hex, and any other shared secret as its
/// bytes. Nothing where it is not written in its form.
#[must_use]
pub fn key(scheme: Scheme, written: &str) -> Option<Vec<u8>> {
    match scheme.key {
        KeyForm::Whsec => written
            .strip_prefix("whsec_")
            .and_then(|encoded| STANDARD.decode(encoded).ok())
            .filter(|key| !key.is_empty()),
        KeyForm::Hex => unhex(written),
        KeyForm::RsaPem => rsa_public_key(written),
        KeyForm::Bytes => (!written.is_empty()).then(|| written.as_bytes().to_vec()),
    }
}

/// The token an `Authorization` header's value carries under the bearer
/// scheme, its scheme named in any case, where it carries one that is not
/// empty.
#[must_use]
pub fn bearer(authorization: &str) -> Option<&str> {
    let (scheme, token) = authorization.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// Whether `presented` is `held`, comparing every byte whatever the first
/// difference, so the time taken says nothing of where they differ.
#[must_use]
pub fn same(held: &[u8], presented: &[u8]) -> bool {
    let differs = held
        .iter()
        .zip(presented)
        .fold(0_u8, |differs, (left, right)| differs | (left ^ right));
    std::hint::black_box(differs) == 0 && held.len() == presented.len()
}

/// The claim of a webhook signing its body alone, delivered as its signature.
fn signed_body(body: &[u8], signature: Vec<u8>) -> Claim {
    let delivery = crate::hex(&signature);
    hmac(body.to_vec(), signature, Some(&delivery))
}

fn hmac(message: Vec<u8>, signature: Vec<u8>, delivery: Option<&str>) -> Claim {
    Claim {
        kind: Signature::HmacSha256,
        message,
        signatures: vec![signature],
        delivery: delivery.map(str::to_owned),
        signer: None,
    }
}

fn header_of<'a>(headers: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}

fn required<'a>(headers: &[(&str, &'a str)], header: &'static str) -> Result<&'a str, Refusal> {
    header_of(headers, header).ok_or(Refusal::Unsigned { header })
}

/// The hex after `prefix` in `header`, as bytes.
fn prefixed(
    headers: &[(&str, &str)],
    header: &'static str,
    prefix: &str,
) -> Result<Vec<u8>, Refusal> {
    required(headers, header)?
        .strip_prefix(prefix)
        .and_then(unhex)
        .ok_or(Refusal::Malformed { header })
}

/// The Unix-seconds timestamp in `header`, as written, where it lies within
/// `window` of `now`.
fn timestamp<'a>(
    headers: &[(&str, &'a str)],
    header: &'static str,
    now: Timestamp,
    window: SignedDuration,
) -> Result<&'a str, Refusal> {
    let written = required(headers, header)?;
    let seconds: i64 = written
        .parse()
        .ok()
        .filter(|_| written.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or(Refusal::Malformed { header })?;
    let at = Timestamp::from_second(seconds).map_err(|_| Refusal::Malformed { header })?;
    if now.duration_since(at).abs() > window {
        return Err(Refusal::Stale { window });
    }
    Ok(written)
}

/// The headers an ActivityPub signature must cover, and `digest` beside
/// them where a body is sent.
/// The pseudo-header a signature names its request's method and path by.
const TARGET: &str = "(request-target)";
const COVERED: [&str; 3] = [TARGET, "host", "date"];

/// The claim of an ActivityPub activity: the signing string of the headers
/// its `Signature` names, with its `Date` inside the replay window and its
/// `Digest` the body's.
/// Where an ActivityPub request was sent, as the driver read it from the
/// request itself and from its config, never from the sender's headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Addressed<'a> {
    /// `<method> <path>`, the method in lowercase and the path with its
    /// query, as `(request-target)` signs it.
    pub target: &'a str,
    /// The host herald serves, from its config, which the signed `host`
    /// must be.
    pub audience: &'a str,
}

/// The claim of an ActivityPub activity sent to `addressed`, with its
/// `headers` and raw `body`, at `now`, within `bounds` (ADR-0007).
///
/// # Errors
///
/// Why it is refused before any signature is checked.
pub fn activity_claim(
    addressed: Addressed<'_>,
    headers: &[(&str, &str)],
    body: &[u8],
    now: Timestamp,
    bounds: &Inbound,
) -> Result<Claim, Refusal> {
    const SIGNATURE: &str = "Signature";
    const DATE: &str = "Date";
    const DIGEST: &str = "Digest";
    let malformed = Refusal::Malformed { header: SIGNATURE };
    let params = parameters(required(headers, SIGNATURE)?).ok_or(malformed.clone())?;
    let param = |name: &str| {
        params
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    };
    if !matches!(param("algorithm"), None | Some(RSA_SHA256 | "hs2019")) {
        return Err(malformed);
    }
    let signer = param("keyId")
        .filter(|signer| !signer.is_empty())
        .ok_or(malformed.clone())?;
    let signature = param("signature")
        .and_then(|encoded| STANDARD.decode(encoded).ok())
        .filter(|signature| !signature.is_empty())
        .ok_or(malformed.clone())?;
    let named = param("headers").unwrap_or("date");
    if named.split(' ').filter(|name| !name.is_empty()).count() > bounds.covered {
        return Err(malformed);
    }
    let covered: Vec<String> = named
        .split(' ')
        .filter(|name| !name.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let digested = (!body.is_empty()).then_some("digest");
    for header in COVERED.into_iter().chain(digested) {
        if !covered.iter().any(|name| name == header) {
            return Err(Refusal::Uncovered { header });
        }
    }
    if !required(headers, "Host")?
        .trim()
        .eq_ignore_ascii_case(addressed.audience)
    {
        return Err(Refusal::Misdirected);
    }
    let date = required(headers, DATE)?;
    let at = jiff::fmt::rfc2822::parse(date)
        .map_err(|_| Refusal::Malformed { header: DATE })?
        .timestamp();
    if now.duration_since(at).abs() > bounds.replay_window {
        return Err(Refusal::Stale {
            window: bounds.replay_window,
        });
    }
    if digested.is_some() {
        let written = required(headers, DIGEST)?;
        let digest = written
            .split(',')
            .find_map(|entry| {
                let (algorithm, value) = entry.trim().split_once('=')?;
                algorithm.eq_ignore_ascii_case("SHA-256").then_some(value)
            })
            .ok_or(Refusal::Malformed { header: DIGEST })?;
        let sent = crate::activity::body_sha256(body);
        if !same(sent.as_bytes(), digest.as_bytes()) {
            return Err(Refusal::Unmatched { header: DIGEST });
        }
    }
    let mut lines = Vec::with_capacity(covered.len());
    for name in &covered {
        let value = match name.as_str() {
            TARGET => Some(addressed.target),
            pseudo if pseudo.starts_with('(') => None,
            name => header_of(headers, name),
        }
        .ok_or(Refusal::Malformed { header: SIGNATURE })?;
        lines.push(format!("{name}: {}", value.trim()));
    }
    let delivery = crate::hex(&signature);
    Ok(Claim {
        kind: Signature::RsaSha256,
        message: lines.join("\n").into_bytes(),
        signatures: vec![signature],
        delivery: Some(delivery),
        signer: Some(signer.to_owned()),
    })
}

/// The `key="value"` pairs of a `Signature` header, each value quoted and
/// the pairs apart by commas; nothing where it is not in that form.
fn parameters(header: &str) -> Option<Vec<(&str, &str)>> {
    let mut found = Vec::new();
    let mut rest = header.trim();
    while !rest.is_empty() {
        let (key, after) = rest.split_once("=\"")?;
        let (value, after) = after.split_once('"')?;
        let key = key.trim();
        if key.is_empty() || found.iter().any(|(seen, _)| *seen == key) {
            return None;
        }
        found.push((key, value));
        rest = after.trim_start();
        match rest.strip_prefix(',') {
            Some(next) => rest = next.trim_start(),
            None if rest.is_empty() => {}
            None => return None,
        }
    }
    (!found.is_empty()).then_some(found)
}

/// The DER of the RSA public key an ActivityPub actor publishes as a PEM
/// `PUBLIC KEY`, as PKCS #1 holds it; nothing where it is not one.
fn rsa_public_key(text: &str) -> Option<Vec<u8>> {
    let der = pem(text, PUBLIC_KEY)?;
    let (info, rest) = element(&der, SEQUENCE)?;
    let (algorithm, info) = element(info, SEQUENCE)?;
    let (bits, after) = element(info, BIT_STRING)?;
    let key = bits.strip_prefix(&[0x00])?;
    (rest.is_empty() && after.is_empty() && algorithm.starts_with(&RSA_ENCRYPTION))
        .then(|| key.to_vec())
        .filter(|key| element(key, SEQUENCE).is_some_and(|(_, after)| after.is_empty()))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod fixtures {
    //! Each source's and destination's scheme as its own crate writes it,
    //! for the core's tests.

    use super::{KeyForm, Scheme, Signed};

    pub(crate) const GITHUB: Scheme = Scheme {
        name: "github",
        signed: Signed::Body {
            headers: &["X-Hub-Signature-256"],
            prefix: "sha256=",
        },
        key: KeyForm::Bytes,
    };

    pub(crate) const FORGEJO: Scheme = Scheme {
        name: "forgejo",
        signed: Signed::Body {
            headers: &["X-Forgejo-Signature", "X-Gitea-Signature"],
            prefix: "",
        },
        key: KeyForm::Bytes,
    };

    pub(crate) const GITLAB: Scheme = Scheme {
        name: "gitlab",
        signed: Signed::StandardWebhooks {
            id: "webhook-id",
            timestamp: "webhook-timestamp",
            signature: "webhook-signature",
        },
        key: KeyForm::Whsec,
    };

    pub(crate) const DISCORD: Scheme = Scheme {
        name: "discord",
        signed: Signed::Ed25519 {
            signature: "X-Signature-Ed25519",
            timestamp: "X-Signature-Timestamp",
        },
        key: KeyForm::Hex,
    };

    pub(crate) const SLACK: Scheme = Scheme {
        name: "slack",
        signed: Signed::Stamped {
            signature: "X-Slack-Signature",
            prefix: "v0=",
            timestamp: "X-Slack-Request-Timestamp",
            version: "v0",
        },
        key: KeyForm::Bytes,
    };

    pub(crate) const MATRIX: Scheme = Scheme {
        name: "matrix",
        signed: Signed::Bearer {
            header: "Authorization",
        },
        key: KeyForm::Bytes,
    };

    pub(crate) const ACTIVITYPUB: Scheme = Scheme {
        name: "activitypub",
        signed: Signed::HttpSignature,
        key: KeyForm::RsaPem,
    };

    pub(crate) const APP: Scheme = Scheme {
        name: "app",
        signed: Signed::Hashed,
        key: KeyForm::Bytes,
    };
}

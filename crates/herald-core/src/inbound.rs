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

/// How far a signed timestamp may lie from now, either way, before the
/// request is taken for a replay.
pub const REPLAY_WINDOW: SignedDuration = SignedDuration::from_mins(5);

/// The most signatures a request may carry. Each is checked in turn, so a
/// request with more is refused before any is checked; a sender rotating its
/// keys signs with two or three.
pub const MOST_SIGNATURES: usize = 4;

/// How long a store keeps a delivery id it admitted (ADR-0013).
pub const DELIVERIES_KEPT: SignedDuration = SignedDuration::from_hours(7 * 24);

/// Who signs a kind of inbound request, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// A GitHub webhook: `X-Hub-Signature-256: sha256=<hex>`, an HMAC-SHA256
    /// of the body.
    GitHub,
    /// A Forgejo or Gitea webhook: `X-Forgejo-Signature` or
    /// `X-Gitea-Signature`, the hex HMAC-SHA256 of the body.
    Forgejo,
    /// A GitLab webhook signed with a signing token, as Standard Webhooks
    /// sign: `webhook-signature: v1,<base64>`, an HMAC-SHA256 of
    /// `<webhook-id>.<webhook-timestamp>.<body>`, keyed by the token's
    /// base64 after its `whsec_` and delivered as `webhook-id`.
    GitLab,
    /// A Discord interaction: `X-Signature-Ed25519` over
    /// `X-Signature-Timestamp` and the body.
    Discord,
    /// A Slack command or interaction: `X-Slack-Signature: v0=<hex>`, an
    /// HMAC-SHA256 of `v0:<X-Slack-Request-Timestamp>:<body>`.
    Slack,
    /// A Matrix application-service transaction: `Authorization: Bearer
    /// <hs_token>`, the homeserver token its registration names.
    Matrix,
}

impl Scheme {
    /// The scheme's name, as a store keeps its deliveries under it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Forgejo => "forgejo",
            Self::GitLab => "gitlab",
            Self::Discord => "discord",
            Self::Slack => "slack",
            Self::Matrix => "matrix",
        }
    }
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
}

/// What a request claims: that one of `signatures` signs `message` under
/// `kind`. A sender rotating its key signs with each key it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// The kind of signature.
    pub kind: Signature,
    /// The bytes that were signed.
    pub message: Vec<u8>,
    /// The signatures over them, any one of which holds.
    pub signatures: Vec<Vec<u8>>,
    /// What makes this delivery that delivery, as its signature covers it,
    /// which the store admits once (ADR-0013): the signed `webhook-id` of a
    /// GitLab webhook, and the signature itself, in hex, of a GitHub or
    /// Forgejo webhook, whose delivery ids no signature covers.
    pub delivery: Option<String>,
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
    /// The signed timestamp lies outside the replay window.
    Stale,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsigned { header } => write!(f, "the request carries no {header}"),
            Self::Malformed { header } => write!(f, "the request's {header} is not in its form"),
            Self::Stale => write!(
                f,
                "the request was signed more than {} minutes from now, so it is taken for a replay",
                REPLAY_WINDOW.as_mins()
            ),
        }
    }
}

/// The claim a request of `scheme` makes, from its `headers` and raw `body`,
/// with `now` bounding any signed timestamp. Header names match in any case.
///
/// # Errors
///
/// The refusal, where a header is missing or malformed, or the signed
/// timestamp lies outside [`REPLAY_WINDOW`].
pub fn claim(
    scheme: Scheme,
    headers: &[(&str, &str)],
    body: &[u8],
    now: Timestamp,
) -> Result<Claim, Refusal> {
    match scheme {
        Scheme::GitHub => {
            const HEADER: &str = "X-Hub-Signature-256";
            let signature = prefixed(headers, HEADER, "sha256=")?;
            Ok(signed_body(body, signature))
        }
        Scheme::Forgejo => {
            let (header, value) = ["X-Forgejo-Signature", "X-Gitea-Signature"]
                .into_iter()
                .find_map(|header| header_of(headers, header).map(|value| (header, value)))
                .ok_or(Refusal::Unsigned {
                    header: "X-Forgejo-Signature",
                })?;
            let signature = unhex(value).ok_or(Refusal::Malformed { header })?;
            Ok(signed_body(body, signature))
        }
        Scheme::Discord => {
            const SIGNATURE: &str = "X-Signature-Ed25519";
            const TIMESTAMP: &str = "X-Signature-Timestamp";
            let stamp = timestamp(headers, TIMESTAMP, now)?;
            let signature = required(headers, SIGNATURE)?;
            Ok(Claim {
                kind: Signature::Ed25519,
                message: [stamp.as_bytes(), body].concat(),
                signatures: vec![unhex(signature).ok_or(Refusal::Malformed { header: SIGNATURE })?],
                delivery: None,
            })
        }
        Scheme::Slack => {
            const TIMESTAMP: &str = "X-Slack-Request-Timestamp";
            let stamp = timestamp(headers, TIMESTAMP, now)?;
            let signature = prefixed(headers, "X-Slack-Signature", "v0=")?;
            Ok(hmac(
                [b"v0:", stamp.as_bytes(), b":", body].concat(),
                signature,
                None,
            ))
        }
        Scheme::Matrix => {
            const HEADER: &str = "Authorization";
            let token = required(headers, HEADER)?
                .strip_prefix("Bearer ")
                .filter(|token| !token.is_empty())
                .ok_or(Refusal::Malformed { header: HEADER })?;
            Ok(Claim {
                kind: Signature::Token,
                message: Vec::new(),
                signatures: vec![token.as_bytes().to_vec()],
                delivery: None,
            })
        }
        Scheme::GitLab => {
            const SIGNATURE: &str = "webhook-signature";
            let id = required(headers, "webhook-id")?;
            let stamp = timestamp(headers, "webhook-timestamp", now)?;
            let signatures: Vec<Vec<u8>> = required(headers, SIGNATURE)?
                .split(' ')
                .filter_map(|entry| entry.strip_prefix("v1,"))
                .map(|encoded| {
                    STANDARD
                        .decode(encoded)
                        .ok()
                        .filter(|signature| !signature.is_empty())
                })
                .take(MOST_SIGNATURES + 1)
                .collect::<Option<_>>()
                .filter(|signatures: &Vec<Vec<u8>>| {
                    (1..=MOST_SIGNATURES).contains(&signatures.len())
                })
                .ok_or(Refusal::Malformed { header: SIGNATURE })?;
            Ok(Claim {
                kind: Signature::HmacSha256,
                message: [id.as_bytes(), b".", stamp.as_bytes(), b".", body].concat(),
                signatures,
                delivery: Some(id.to_owned()),
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
    match scheme {
        Scheme::GitLab => written
            .strip_prefix("whsec_")
            .and_then(|encoded| STANDARD.decode(encoded).ok())
            .filter(|key| !key.is_empty()),
        Scheme::Discord => unhex(written),
        Scheme::GitHub | Scheme::Forgejo | Scheme::Slack | Scheme::Matrix => {
            (!written.is_empty()).then(|| written.as_bytes().to_vec())
        }
    }
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
/// [`REPLAY_WINDOW`] of `now`.
fn timestamp<'a>(
    headers: &[(&str, &'a str)],
    header: &'static str,
    now: Timestamp,
) -> Result<&'a str, Refusal> {
    let written = required(headers, header)?;
    let seconds: i64 = written
        .parse()
        .ok()
        .filter(|_| written.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or(Refusal::Malformed { header })?;
    let at = Timestamp::from_second(seconds).map_err(|_| Refusal::Malformed { header })?;
    if now.duration_since(at).abs() > REPLAY_WINDOW {
        return Err(Refusal::Stale);
    }
    Ok(written)
}

/// The bytes `text` spells in hex, in either case; nothing where it spells
/// none.
fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .filter(|pair| pair.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect()
}

#[cfg(test)]
mod tests;

//! What an inbound request claims about who signed it, read from its raw
//! headers and body before anything parses the body (ADR-0007).
//!
//! The claim names the bytes that were signed and the signature over them.
//! Checking the signature is left to a `Verifier`, so this step holds no key.

use std::fmt;

use jiff::{SignedDuration, Timestamp};

/// How far a signed timestamp may lie from now, either way, before the
/// request is taken for a replay.
pub const REPLAY_WINDOW: SignedDuration = SignedDuration::from_mins(5);

/// Who signs a kind of inbound request, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// A GitHub webhook: `X-Hub-Signature-256: sha256=<hex>`, an HMAC-SHA256
    /// of the body.
    GitHub,
    /// A Forgejo or Gitea webhook: `X-Forgejo-Signature` or
    /// `X-Gitea-Signature`, the hex HMAC-SHA256 of the body.
    Forgejo,
    /// A Discord interaction: `X-Signature-Ed25519` over
    /// `X-Signature-Timestamp` and the body.
    Discord,
    /// A Slack command or interaction: `X-Slack-Signature: v0=<hex>`, an
    /// HMAC-SHA256 of `v0:<X-Slack-Request-Timestamp>:<body>`.
    Slack,
}

/// The kind of signature a scheme carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signature {
    /// HMAC over SHA-256, keyed by a shared secret.
    HmacSha256,
    /// Ed25519, checked with the sender's public key.
    Ed25519,
}

/// What a request claims: that `signature` signs `message` under `kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// The kind of signature.
    pub kind: Signature,
    /// The bytes that were signed.
    pub message: Vec<u8>,
    /// The signature over them.
    pub signature: Vec<u8>,
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
            Ok(hmac(body.to_vec(), signature))
        }
        Scheme::Forgejo => {
            let (header, value) = ["X-Forgejo-Signature", "X-Gitea-Signature"]
                .into_iter()
                .find_map(|header| header_of(headers, header).map(|value| (header, value)))
                .ok_or(Refusal::Unsigned {
                    header: "X-Forgejo-Signature",
                })?;
            Ok(hmac(
                body.to_vec(),
                unhex(value).ok_or(Refusal::Malformed { header })?,
            ))
        }
        Scheme::Discord => {
            const SIGNATURE: &str = "X-Signature-Ed25519";
            const TIMESTAMP: &str = "X-Signature-Timestamp";
            let stamp = timestamp(headers, TIMESTAMP, now)?;
            let signature = required(headers, SIGNATURE)?;
            Ok(Claim {
                kind: Signature::Ed25519,
                message: [stamp.as_bytes(), body].concat(),
                signature: unhex(signature).ok_or(Refusal::Malformed { header: SIGNATURE })?,
            })
        }
        Scheme::Slack => {
            const TIMESTAMP: &str = "X-Slack-Request-Timestamp";
            let stamp = timestamp(headers, TIMESTAMP, now)?;
            let signature = prefixed(headers, "X-Slack-Signature", "v0=")?;
            Ok(hmac(
                [b"v0:", stamp.as_bytes(), b":", body].concat(),
                signature,
            ))
        }
    }
}

fn hmac(message: Vec<u8>, signature: Vec<u8>) -> Claim {
    Claim {
        kind: Signature::HmacSha256,
        message,
        signature,
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

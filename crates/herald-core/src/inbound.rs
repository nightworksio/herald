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
    /// An ActivityPub activity posted to herald's inbox, signed as HTTP
    /// Signatures do: `Signature` names the signer's `keyId` and the headers
    /// it signs, which must hold `(request-target)`, `host`, `date` and, with
    /// a body, `digest`; `Digest: SHA-256=<base64>` is the body's. It is
    /// claimed by [`activity_claim`], with where it was sent, and delivered
    /// as its signature.
    ActivityPub,
    /// An app's event posted to herald's HTTP API with `Authorization:
    /// Bearer <token>`, checked against the hashes `[apps]` keeps and
    /// delivered as the event's identity (ADR-0007).
    App,
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
            Self::ActivityPub => "activitypub",
            Self::App => "app",
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
    /// The signed timestamp lies outside the replay window.
    Stale,
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
            Self::Stale => write!(
                f,
                "the request was signed more than {} minutes from now, so it is taken for a replay",
                REPLAY_WINDOW.as_mins()
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
            let signature = unhex(required(headers, SIGNATURE)?)
                .ok_or(Refusal::Malformed { header: SIGNATURE })?;
            Ok(Claim {
                kind: Signature::Ed25519,
                message: [stamp.as_bytes(), body].concat(),
                delivery: Some(crate::hex(&signature)),
                signatures: vec![signature],
                signer: None,
            })
        }
        Scheme::Slack => {
            const TIMESTAMP: &str = "X-Slack-Request-Timestamp";
            let stamp = timestamp(headers, TIMESTAMP, now)?;
            let signature = prefixed(headers, "X-Slack-Signature", "v0=")?;
            let delivery = crate::hex(&signature);
            Ok(hmac(
                [b"v0:", stamp.as_bytes(), b":", body].concat(),
                signature,
                Some(&delivery),
            ))
        }
        Scheme::ActivityPub | Scheme::App => Err(Refusal::Unaddressed),
        Scheme::Matrix => {
            const HEADER: &str = "Authorization";
            let token =
                bearer(required(headers, HEADER)?).ok_or(Refusal::Malformed { header: HEADER })?;
            Ok(Claim {
                kind: Signature::Token,
                message: Vec::new(),
                signatures: vec![token.as_bytes().to_vec()],
                delivery: None,
                signer: None,
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
    match scheme {
        Scheme::GitLab => written
            .strip_prefix("whsec_")
            .and_then(|encoded| STANDARD.decode(encoded).ok())
            .filter(|key| !key.is_empty()),
        Scheme::Discord => unhex(written),
        Scheme::ActivityPub => rsa_public_key(written),
        Scheme::GitHub | Scheme::Forgejo | Scheme::Slack | Scheme::Matrix | Scheme::App => {
            (!written.is_empty()).then(|| written.as_bytes().to_vec())
        }
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

/// The headers an ActivityPub signature must cover, and `digest` beside
/// them where a body is sent.
/// The pseudo-header a signature names its request's method and path by.
const TARGET: &str = "(request-target)";
const COVERED: [&str; 3] = [TARGET, "host", "date"];

/// The most headers an ActivityPub signature may name. Each is read into the
/// signing string before the signature is checked, so a request naming more
/// is refused unread.
pub const MOST_COVERED: usize = 16;

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
/// `headers` and raw `body`, at `now` (ADR-0007).
///
/// # Errors
///
/// Why it is refused before any signature is checked.
pub fn activity_claim(
    addressed: Addressed<'_>,
    headers: &[(&str, &str)],
    body: &[u8],
    now: Timestamp,
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
    if !matches!(param("algorithm"), None | Some("rsa-sha256" | "hs2019")) {
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
    if named.split(' ').filter(|name| !name.is_empty()).count() > MOST_COVERED {
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
    if now.duration_since(at).abs() > REPLAY_WINDOW {
        return Err(Refusal::Stale);
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
fn rsa_public_key(pem: &str) -> Option<Vec<u8>> {
    const RSA_ENCRYPTION: [u8; 11] = [
        0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01,
    ];
    let encoded: String = pem
        .trim()
        .strip_prefix("-----BEGIN PUBLIC KEY-----")?
        .strip_suffix("-----END PUBLIC KEY-----")?
        .split_whitespace()
        .collect();
    let der = STANDARD.decode(encoded).ok()?;
    let (info, rest) = element(&der, 0x30)?;
    let (algorithm, info) = element(info, 0x30)?;
    let (bits, after) = element(info, 0x03)?;
    let key = bits.strip_prefix(&[0x00])?;
    (rest.is_empty() && after.is_empty() && algorithm.starts_with(&RSA_ENCRYPTION))
        .then(|| key.to_vec())
        .filter(|key| element(key, 0x30).is_some_and(|(_, after)| after.is_empty()))
}

/// The contents of the DER element of tag `tag` that `der` opens with, and
/// what follows it. A length is read in one byte, or in the one or two after
/// `0x81` or `0x82`, which an RSA key's lengths fit.
fn element(der: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (&found, rest) = der.split_first()?;
    let (length, rest) = match rest {
        [short @ 0..=0x7f, rest @ ..] => (usize::from(*short), rest),
        [0x81, long, rest @ ..] => (usize::from(*long), rest),
        [0x82, high, low, rest @ ..] => (usize::from(u16::from_be_bytes([*high, *low])), rest),
        _ => return None,
    };
    (found == tag && rest.len() >= length).then(|| rest.split_at(length))
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

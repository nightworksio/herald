use std::collections::BTreeMap;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jiff::{SignedDuration, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::claim_pattern::ClaimPattern;
use crate::{KeptToken, digest};

/// What a request for an ID token names the audience it is meant for.
const AUDIENCE: &str = "audience";

/// What a CI's answer to a request for an ID token carries it under.
const ANSWERED: &str = "value";

/// The algorithm an ID token is signed with, of those herald checks.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum TokenAlgorithm {
    /// RSASSA-PKCS1-v1_5 with SHA-256.
    #[serde(rename = "RS256")]
    Rs256,
}

impl TokenAlgorithm {
    /// Every algorithm herald checks.
    pub const ALL: [Self; 1] = [Self::Rs256];

    /// Its name, as a token's header and the config write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rs256 => "RS256",
        }
    }

    /// The algorithm `name` names, of those herald checks.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|algorithm| algorithm.name() == name)
    }
}

/// An ID token read from its compact form, its signature not yet checked.
#[derive(Clone, PartialEq)]
pub struct IdToken {
    algorithm: String,
    key_id: Option<String>,
    claims: Map<String, Value>,
    signed: Vec<u8>,
    signature: Vec<u8>,
}

/// Why an ID token is not taken. None of them quotes the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Untaken {
    /// It is not a compact JWS of a JSON header and JSON claims.
    Unreadable,
    /// It is signed with an algorithm its entry does not allow.
    Algorithm,
    /// No entry names its issuer.
    Issuer,
    /// It is not meant for this herald.
    Audience,
    /// One of its times does not hold, as this says: it expired, is not
    /// valid yet, names no expiry or issue, or lives too long.
    Time(&'static str),
    /// Its issuer's keys hold none it names.
    Key,
    /// Its signature does not hold.
    Signature,
    /// The claim of this name does not match its entry.
    Claim(String),
    /// Its issuer's keys could not be read: why.
    Keys(String),
    /// It was taken before, and each is taken once.
    Used,
    /// Whether it was taken before could not be read or kept: why.
    Unkept(String),
}

impl Untaken {
    /// Why a token names no expiry.
    pub const NO_EXPIRY: &str = "names no expiry";
    /// Why a token names no time it was issued.
    pub const NO_ISSUE: &str = "names no time it was issued";
    /// Why a token's time is not read.
    pub const UNWHOLE: &str = "names a time that is not a whole number of seconds since 1970";
    /// Why a token past its expiry is refused.
    pub const EXPIRED: &str = "has expired";
    /// Why a token before its `nbf` is refused.
    pub const EARLY: &str = "is not valid yet";
    /// Why a token issued later than now is refused.
    pub const AHEAD: &str = "was issued later than now";
    /// Why a token expiring before it was issued is refused.
    pub const BACKWARD: &str = "expires before it was issued";
    /// Why a token living longer than its entry allows is refused.
    pub const LONG: &str = "lives longer than its entry's lifetime allows";

    /// Whether herald cannot tell now whether the token holds, rather
    /// than knowing it does not.
    #[must_use]
    pub const fn unavailable(&self) -> bool {
        matches!(self, Self::Keys(_) | Self::Unkept(_))
    }
}

impl fmt::Display for Untaken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("the ID token cannot be read"),
            Self::Algorithm => f.write_str(
                "the ID token is signed with an algorithm its issuer's entry does not allow",
            ),
            Self::Issuer => f.write_str("no app takes ID tokens of the token's issuer"),
            Self::Audience => f.write_str("the ID token is not meant for this herald"),
            Self::Time(why) => write!(f, "the ID token {why}"),
            Self::Key => f.write_str("the ID token's issuer publishes no key it names"),
            Self::Signature => f.write_str("the ID token's signature does not hold"),
            Self::Claim(name) => write!(
                f,
                "the ID token's claim {name} does not match its app's entry"
            ),
            Self::Keys(why) => write!(f, "the ID token's issuer's keys could not be read: {why}"),
            Self::Used => f.write_str("the ID token was taken before, and each is taken once"),
            Self::Unkept(why) => write!(
                f,
                "whether the ID token was taken before could not be kept: {why}"
            ),
        }
    }
}

impl fmt::Debug for IdToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdToken")
            .field("algorithm", &self.algorithm)
            .field("key_id", &self.key_id)
            .field("issuer", &self.issuer())
            .finish_non_exhaustive()
    }
}

impl IdToken {
    /// The claim naming the token's issuer.
    pub const ISSUER: &str = "iss";

    /// The most bytes an ID token is read from.
    pub const MOST: usize = 16 * 1024;

    /// The claims a token's evidence keeps wherever the token carries
    /// them, beside those its entry binds: its id, its subject, the commit,
    /// and the run, job and workflow GitHub Actions and GitLab CI name.
    pub const EVIDENCED: [&str; 9] = [
        "jti",
        "sub",
        "sha",
        "run_id",
        "run_attempt",
        "job_workflow_ref",
        "pipeline_id",
        "job_id",
        "ci_config_ref_uri",
    ];

    /// What follows the address `url` a CI takes requests for ID tokens
    /// at, so it is asked for one meant for `audience`: the audience as
    /// the query's next part, or as the query where it has none.
    #[must_use]
    pub fn asking(url: &str, audience: &str) -> String {
        let joined = if url.contains('?') { '&' } else { '?' };
        let audience: String = url::form_urlencoded::byte_serialize(audience.as_bytes()).collect();
        format!("{joined}{AUDIENCE}={audience}")
    }

    /// The token a CI's answer `body` to a request for one carries under
    /// `value`, as GitHub Actions answers.
    #[must_use]
    pub fn answered(body: &[u8]) -> Option<String> {
        match serde_json::from_slice::<Value>(body).ok()?.get(ANSWERED)? {
            Value::String(token) => Some(token.clone()),
            _ => None,
        }
    }

    /// Whether `text` is shaped as a compact JWS: three parts, two dots.
    #[must_use]
    pub fn shaped(text: &str) -> bool {
        text.bytes().filter(|&byte| byte == b'.').count() == 2
    }

    /// The token `text` writes.
    ///
    /// # Errors
    ///
    /// Where it is longer than [`Self::MOST`], is not three base64url parts,
    /// or its header or claims are not JSON objects, or its header names no
    /// algorithm.
    pub fn read(text: &str) -> Result<Self, Untaken> {
        if text.len() > Self::MOST {
            return Err(Untaken::Unreadable);
        }
        let mut parts = text.split('.');
        let (Some(header), Some(claims), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(Untaken::Unreadable);
        };
        let object = |part: &str| match decoded(part)
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        {
            Some(Value::Object(object)) => Ok(object),
            _ => Err(Untaken::Unreadable),
        };
        let head = object(header)?;
        let algorithm = match head.get("alg") {
            Some(Value::String(algorithm)) => algorithm.clone(),
            _ => return Err(Untaken::Unreadable),
        };
        let key_id = match head.get("kid") {
            None => None,
            Some(Value::String(kid)) => Some(kid.clone()),
            Some(_) => return Err(Untaken::Unreadable),
        };
        let signature = decoded(signature)
            .filter(|bytes| !bytes.is_empty())
            .ok_or(Untaken::Unreadable)?;
        Ok(Self {
            algorithm,
            key_id,
            claims: object(claims)?,
            signed: format!("{header}.{claims}").into_bytes(),
            signature,
        })
    }

    /// Its issuer, where it names one.
    #[must_use]
    pub fn issuer(&self) -> Option<&str> {
        self.claims.get(Self::ISSUER).and_then(Value::as_str)
    }

    /// When it was issued and when it expires, where it says both in
    /// whole seconds.
    #[must_use]
    pub fn valid(&self) -> Option<(Timestamp, Timestamp)> {
        let at = |name: &str| {
            self.claims
                .get(name)
                .and_then(Value::as_u64)
                .and_then(|seconds| i64::try_from(seconds).ok())
                .and_then(|seconds| Timestamp::from_second(seconds).ok())
        };
        Some((at("iat")?, at("exp")?))
    }

    /// What it is remembered by once taken: its issuer's digest, and the
    /// digest of its `jti`, or where it names none, of its signed bytes and
    /// signature; so no part of it is kept, and no other encoding of it
    /// is remembered apart.
    #[must_use]
    pub fn remembered_as(&self) -> String {
        let used = match self.claims.get("jti").and_then(Value::as_str) {
            Some(id) => digest(&format!("jti\n{id}")),
            None => digest(&format!(
                "token\n{}\n{}",
                String::from_utf8_lossy(&self.signed),
                URL_SAFE_NO_PAD.encode(&self.signature)
            )),
        };
        format!("{}/{used}", digest(self.issuer().unwrap_or_default()))
    }

    /// The key it names, where it names one.
    #[must_use]
    pub fn key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }

    /// The bytes its signature is over.
    #[must_use]
    pub fn signed(&self) -> &[u8] {
        &self.signed
    }

    /// Its signature.
    #[must_use]
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }

    /// The algorithm it is signed with, where it is one of `allowed`.
    ///
    /// # Errors
    ///
    /// Where its header names any other.
    pub fn algorithm(&self, allowed: &[TokenAlgorithm]) -> Result<TokenAlgorithm, Untaken> {
        TokenAlgorithm::named(&self.algorithm)
            .filter(|algorithm| allowed.contains(algorithm))
            .ok_or(Untaken::Algorithm)
    }

    /// Whether its `aud` names `audience`, alone or among others.
    #[must_use]
    pub fn meant_for(&self, audience: &str) -> bool {
        match self.claims.get("aud") {
            Some(Value::String(named)) => named == audience,
            Some(Value::Array(named)) => named.iter().any(|named| named.as_str() == Some(audience)),
            _ => false,
        }
    }

    /// Whether it is meant for `audience` and valid at `now`, give or take
    /// `skew`, living no longer than `lifetime`: its `aud` names the
    /// audience, its `exp` is still ahead, its `iat` and any `nbf` are not
    /// ahead, every one of them in whole seconds, and from `iat` to `exp`
    /// is no longer than `lifetime`.
    ///
    /// # Errors
    ///
    /// Where it names another audience, no expiry or issue, a time that
    /// is not whole seconds, a time outside them, or a longer life.
    pub fn held_to(
        &self,
        audience: &str,
        now: Timestamp,
        (skew, lifetime): (SignedDuration, SignedDuration),
    ) -> Result<(), Untaken> {
        if !self.meant_for(audience) {
            return Err(Untaken::Audience);
        }
        let whole = |name: &str| {
            self.claims
                .get(name)
                .map(|value| {
                    value
                        .as_u64()
                        .and_then(|seconds| i64::try_from(seconds).ok())
                        .ok_or(Untaken::Time(Untaken::UNWHOLE))
                })
                .transpose()
        };
        let expires = whole("exp")?.ok_or(Untaken::Time(Untaken::NO_EXPIRY))?;
        let issued = whole("iat")?.ok_or(Untaken::Time(Untaken::NO_ISSUE))?;
        let valid = whole("nbf")?;
        let now = now.as_second();
        let skew = skew.as_secs();
        let why = if expires.saturating_add(skew) <= now {
            Untaken::EXPIRED
        } else if valid.is_some_and(|valid| valid.saturating_sub(skew) > now) {
            Untaken::EARLY
        } else if issued.saturating_sub(skew) > now {
            Untaken::AHEAD
        } else if expires < issued {
            Untaken::BACKWARD
        } else if expires - issued > lifetime.as_secs() {
            Untaken::LONG
        } else {
            return Ok(());
        };
        Err(Untaken::Time(why))
    }

    /// The claim `name`, written as text: a string as it is, a number or a
    /// flag as JSON writes it, and nothing for any other value.
    #[must_use]
    pub fn claim(&self, name: &str) -> Option<String> {
        match self.claims.get(name)? {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            Value::Bool(flag) => Some(flag.to_string()),
            _ => None,
        }
    }

    /// Whether every claim `wanted` names matches its pattern.
    ///
    /// # Errors
    ///
    /// The first claim, by name, that is absent or does not match.
    pub fn matching(&self, wanted: &BTreeMap<String, ClaimPattern>) -> Result<(), Untaken> {
        match wanted.iter().find(|(name, pattern)| {
            self.claim(name)
                .is_none_or(|claim| !pattern.matches(&claim))
        }) {
            Some((name, _)) => Err(Untaken::Claim(name.clone())),
            None => Ok(()),
        }
    }

    /// What it says of the run it was issued to, as evidence: its issuer,
    /// each claim of [`Self::EVIDENCED`] it carries and each claim `bound`
    /// names, written as text; the token itself never.
    #[must_use]
    pub fn evidence(&self, bound: &BTreeMap<String, ClaimPattern>) -> BTreeMap<String, String> {
        std::iter::once(Self::ISSUER)
            .chain(Self::EVIDENCED)
            .chain(bound.keys().map(String::as_str))
            .filter_map(|name| Some((name.to_owned(), self.claim(name)?)))
            .collect()
    }
}

/// The RSA keys an issuer publishes for its tokens, each by the id it gives
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeySet {
    keys: Vec<RsaKey>,
}

/// One RSA public key of an issuer's key set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsaKey {
    /// The id the issuer gives it.
    pub id: Option<String>,
    /// Its modulus, big-endian.
    pub modulus: Vec<u8>,
    /// Its public exponent, big-endian.
    pub exponent: Vec<u8>,
}

impl KeySet {
    /// The RSA signing keys of the JSON Web Key Set `body`, skipping every
    /// key of another type, use or algorithm.
    ///
    /// # Errors
    ///
    /// Where the body is not a JSON object holding a `keys` array.
    pub fn read(body: &[u8]) -> Result<Self, Untaken> {
        let unread = || Untaken::Keys("the key set is not a JSON Web Key Set".to_owned());
        let Ok(Value::Object(set)) = serde_json::from_slice::<Value>(body) else {
            return Err(unread());
        };
        let Some(Value::Array(listed)) = set.get("keys") else {
            return Err(unread());
        };
        let text = |key: &Map<String, Value>, name: &str| {
            key.get(name).and_then(Value::as_str).map(str::to_owned)
        };
        let keys = listed
            .iter()
            .filter_map(Value::as_object)
            .filter(|key| text(key, "kty").as_deref() == Some("RSA"))
            .filter(|key| text(key, "use").is_none_or(|used| used == "sig"))
            .filter(|key| {
                text(key, "alg").is_none_or(|algorithm| algorithm == TokenAlgorithm::Rs256.name())
            })
            .filter_map(|key| {
                Some(RsaKey {
                    id: text(key, "kid"),
                    modulus: decoded(&text(key, "n")?)?,
                    exponent: decoded(&text(key, "e")?)?,
                })
            })
            .collect();
        Ok(Self { keys })
    }

    /// The key `id` names; with no id, its only key.
    #[must_use]
    pub fn key(&self, id: Option<&str>) -> Option<&RsaKey> {
        match id {
            Some(id) => self.keys.iter().find(|key| key.id.as_deref() == Some(id)),
            None => match self.keys.as_slice() {
                [only] => Some(only),
                _ => None,
            },
        }
    }

    /// Whether it holds no key.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// An issuer's key set as the store keeps it: as read and when, and when
/// it was last fetched or tried, so it is fetched at most once in a
/// cooldown whatever tokens name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptKeys {
    keys: Value,
    /// When the keys were read; none where none ever was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched: Option<Timestamp>,
    /// When they were last fetched, or tried.
    pub tried: Timestamp,
}

impl KeptKeys {
    /// The key set `body`, read at `now`.
    #[must_use]
    pub fn new(body: &[u8], now: Timestamp) -> Self {
        Self {
            keys: serde_json::from_slice(body).unwrap_or_default(),
            fetched: Some(now),
            tried: now,
        }
    }

    /// No key set, tried at `now`.
    #[must_use]
    pub const fn untried(now: Timestamp) -> Self {
        Self {
            keys: Value::Null,
            fetched: None,
            tried: now,
        }
    }

    /// It, tried again at `now` and kept as it was.
    #[must_use]
    pub fn tried_at(self, now: Timestamp) -> Self {
        Self { tried: now, ..self }
    }

    /// Its keys; none where they do not read.
    #[must_use]
    pub fn keys(&self) -> KeySet {
        serde_json::to_vec(&self.keys)
            .ok()
            .and_then(|body| KeySet::read(&body).ok())
            .unwrap_or_default()
    }

    /// The key set `token` keeps, where it reads.
    #[must_use]
    pub fn read(token: &KeptToken) -> Option<Self> {
        serde_json::from_slice(&token.sealed_bytes()?).ok()
    }

    /// It as the record `name`, its keys used until `expires`.
    #[must_use]
    pub fn kept(&self, name: String, expires: Timestamp) -> KeptToken {
        KeptToken::new(name, &serde_json::to_vec(self).unwrap_or_default(), expires)
    }
}

/// The key set address the discovery document `body` of an issuer names,
/// where the document is the issuer's own and the address is `https` on the
/// issuer's origin.
///
/// # Errors
///
/// Where the body names another issuer, no `jwks_uri`, or one elsewhere.
pub fn jwks_uri(body: &[u8], issuer: &str) -> Result<String, Untaken> {
    let refused = |why: &str| Untaken::Keys(why.to_owned());
    let Ok(Value::Object(document)) = serde_json::from_slice::<Value>(body) else {
        return Err(refused("the discovery document is not a JSON object"));
    };
    if document.get("issuer").and_then(Value::as_str) != Some(issuer) {
        return Err(refused("the discovery document names another issuer"));
    }
    let named = document
        .get("jwks_uri")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("the discovery document names no jwks_uri"))?;
    if !same_origin(named, issuer) {
        return Err(refused(
            "the discovery document's jwks_uri is not https on the issuer's own origin",
        ));
    }
    Ok(named.to_owned())
}

/// Where an issuer publishes its discovery document.
#[must_use]
pub fn discovery_of(issuer: &str) -> String {
    format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    )
}

/// Whether `url` is `https` on the origin of `issuer`.
#[must_use]
pub fn same_origin(url: &str, issuer: &str) -> bool {
    match (url::Url::parse(url), url::Url::parse(issuer)) {
        (Ok(url), Ok(issuer)) => {
            url.scheme() == "https"
                && url.origin() == issuer.origin()
                && url.username().is_empty()
                && url.password().is_none()
        }
        _ => false,
    }
}

fn decoded(part: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(part).ok()
}

#[cfg(test)]
mod tests;

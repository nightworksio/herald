//! A request posted to a destination's endpoint on a bearer token its
//! sender's issuer signs, as the Bot Framework vouches for each activity it
//! posts to a bot: its body's size bounded, then its token read and its
//! algorithm, issuer, audience and times checked, then its signature
//! checked by a key the issuer's metadata document lists and endorses for
//! what herald takes, all before anything parses the body (ADR-0007,
//! ADR-0093). The token is never kept, logged or quoted.

use herald_core::{
    IdToken, KeySet, TokenAlgorithm, Untaken, bearer, digest, jwks_uri_on, signs_with,
};
use herald_kinds::{Vouched, Vouching};
use herald_ports::{Http, Store, Verifier};
use jiff::Timestamp;

use crate::oidc::signed_with;
use crate::published::{Publisher, got, keys};
use crate::verification::Rejected;

/// Where the store keeps the keys an issuer vouches with, before the
/// digest of its metadata's address.
const KEPT: &str = "vouching/";

/// The algorithm a vouching token is signed with: the one herald checks.
const ALGORITHM: TokenAlgorithm = TokenAlgorithm::Rs256;

/// Why a request carrying no bearer token is refused.
const UNBORNE: &str = "is missing";
/// Why a token that is not a compact JWS of JSON is refused.
const UNREADABLE: &str = "cannot be read";
/// Why a token signed with another algorithm is refused.
const ALGORITHMIC: &str = "is signed with an algorithm herald does not check";
/// Why a token of another issuer is refused.
const ISSUED: &str = "names another issuer";
/// Why a token meant for another audience is refused.
const MEANT: &str = "is meant for another audience";
/// Why a token naming no key, or one its issuer does not publish, is
/// refused.
const UNKEYED: &str = "names no key its issuer publishes";
/// Why a token lacking a claim its request is read with is refused.
const UNCLAIMED: &str = "carries no claim its request is read with";

/// The claims of the bearer token `headers` carry, as `vouching` holds
/// it at `now`, its body `body` no longer than `vouching` lets one be;
/// its issuer's keys read through `http` and kept in `store`, and its
/// signature checked by `verifier`.
///
/// # Errors
///
/// Why it is not let through to a parser.
pub(crate) async fn vouched<S: Store, H: Http, V: Verifier>(
    (store, http, verifier): (&S, &H, &V),
    vouching: &Vouching,
    (headers, body): (&[(&str, &str)], &[u8]),
    now: Timestamp,
) -> Result<Vouched, Rejected> {
    if body.len() > vouching.body {
        return Err(Rejected::Oversized);
    }
    let carried = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, value)| bearer(value))
        .ok_or(Rejected::Unvouched(UNBORNE))?;
    let token = IdToken::read(carried).map_err(|_| Rejected::Unvouched(UNREADABLE))?;
    token
        .algorithm(&[ALGORITHM])
        .map_err(|_| Rejected::Unvouched(ALGORITHMIC))?;
    if token.issuer() != Some(vouching.issuer.as_str()) {
        return Err(Rejected::Unvouched(ISSUED));
    }
    if !token.meant_for(&vouching.audience) {
        return Err(Rejected::Unvouched(MEANT));
    }
    token
        .within(now, vouching.skew)
        .map_err(Rejected::Unvouched)?;
    let named = token.key_id().ok_or(Rejected::Unvouched(UNKEYED))?;
    let kept = format!("{KEPT}{}", digest(&vouching.metadata));
    let publisher = Publisher {
        kept: &kept,
        pinned: None,
        keys_for: vouching.keys_for,
        keys_retry: vouching.keys_retry,
    };
    let keys = keys(store, publisher, (Some(named), now), || {
        fetched(http, vouching)
    })
    .await
    .map_err(|untaken| Rejected::Unkept {
        reason: untaken.to_string(),
    })?;
    if keys.unendorsed(Some(named)) {
        return Err(Rejected::Unendorsed);
    }
    let key = keys.key(Some(named)).ok_or(Rejected::Unvouched(UNKEYED))?;
    let public = herald_keys::rsa_public(&key.modulus, &key.exponent);
    if !verifier
        .verify(
            signed_with(ALGORITHM),
            &public,
            token.signed(),
            token.signature(),
        )
        .await
    {
        return Err(Rejected::Forged);
    }
    let claims = vouching
        .claims
        .iter()
        .map(|name| Some((*name, token.claim(name)?)))
        .collect::<Option<Vec<_>>>()
        .ok_or(Rejected::Unvouched(UNCLAIMED))?;
    Ok(Vouched { claims })
}

/// The keys `vouching`'s issuer endorses for what herald takes, read
/// through `http` from the key set its metadata document names on the
/// metadata's own origin, where the metadata is that issuer's and lists the
/// algorithm herald checks, and that set written as herald keeps it.
///
/// # Errors
///
/// An answer is not 200 or too large, the metadata is another issuer's,
/// names keys elsewhere or lists no algorithm herald checks, or the key
/// set lists too many keys or none endorsed.
async fn fetched<H: Http>(http: &H, vouching: &Vouching) -> Result<(KeySet, Vec<u8>), Untaken> {
    let metadata = got(http, &vouching.metadata, vouching.keys_most).await?;
    let address = jwks_uri_on(&metadata, &vouching.issuer, &vouching.metadata)?;
    if !signs_with(&metadata, ALGORITHM) {
        return Err(Untaken::Keys(format!(
            "{} lists no signing algorithm herald checks",
            vouching.metadata
        )));
    }
    let keys = KeySet::read(&got(http, &address, vouching.keys_most).await?)?
        .endorsed(vouching.endorsement);
    if keys.is_empty() {
        return Err(Untaken::Keys(format!(
            "{address} lists no key endorsed for {}",
            vouching.endorsement
        )));
    }
    let written = keys.written();
    Ok((keys, written))
}

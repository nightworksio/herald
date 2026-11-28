//! A CI run's ID token taken as a credential (ADR-0064): read, its
//! algorithm and its issuer checked against the apps' entries, and held to
//! an entry's audience and times, all before any store read or fetch; then
//! its signature checked by a key of the issuer's, and its claims matched
//! against the entry's patterns. An issuer's keys are pinned, or read from
//! the address configured for it and kept in the store for `keys_for`,
//! fetched again for a key they lack at most once in `keys_retry` however
//! many tokens name one. The token is never kept, logged or quoted; only
//! its issuer, its id and run, and the claims an entry binds are. Where an
//! app takes each token once, a token taken is remembered among the
//! store's deliveries before anything it brings is taken, until it
//! expires, and refused a second time.

use std::collections::{BTreeMap, BTreeSet};

use herald_config::{App, Config, Oidc, Replay, Taking};
use herald_core::{IdToken, KeySet, TokenAlgorithm, Untaken, digest, discovery_of, jwks_uri};
use herald_ports::{Admitted, Algorithm, Deliveries, DynVerifier, Erased, Http, Store, Verifier};
use jiff::Timestamp;

use crate::published::{Publisher, got, keys};

/// The most bytes an issuer's discovery document or key set is read from.
pub(crate) const KEYS_MOST: usize = 64 * 1024;

/// Where the store keeps an issuer's keys, before its digest.
const KEPT: &str = "oidc/";

/// The namespace each token taken once is remembered under among the
/// store's deliveries.
pub(crate) const TAKEN: &str = "oidc";

/// Why an issuer's keys are not read where this herald reaches no address
/// of its.
const UNREACHED: &str = "they are read from no address this herald reaches";

/// An entry naming a token's issuer that the token is held to, its app by
/// name, and the algorithm the token is signed with.
type Held<'c> = (&'c str, &'c App, &'c Oidc, TokenAlgorithm);

/// An ID token taken: the app whose entry took it, the entry, and what it
/// says of its run.
#[derive(Debug)]
pub(crate) struct Taken<'c> {
    /// The app.
    pub(crate) app: &'c str,
    /// The names the app stands for.
    pub(crate) stands_for: &'c BTreeSet<String>,
    /// The entry.
    pub(crate) entry: &'c Oidc,
    /// The token's issuer, its id and run, and each claim the entry binds.
    pub(crate) evidence: BTreeMap<String, String>,
    /// When the token was issued and when it expires.
    pub(crate) valid: (Timestamp, Timestamp),
    /// How often the app takes one token.
    pub(crate) replay: Replay,
}

/// The ID token `bearer` taken, to do `taking`, by the first app of
/// `config`, by name, whose entry that lets its tokens do it holds it,
/// its issuer's keys read through the connection `issuers` holds for it
/// and kept in `store`; and where the app takes each token once,
/// remembered in `store` first, unless `again` it is worked once more
/// after it was answered. Or why it is not taken.
///
/// # Errors
///
/// It cannot be read, no entry names its issuer, it is held to none, its
/// issuer's keys cannot be read or hold no key it names, its signature
/// does not hold, its claims match no entry it is held to, or it was
/// taken before by an app taking each once, or that cannot be told.
pub(crate) async fn taken<'c, S: Store, H: Http>(
    (config, store, verifier): (&'c Config, &S, &dyn DynVerifier),
    issuers: &[(String, H)],
    (bearer, taking, again): (&str, Taking, bool),
    now: Timestamp,
) -> Result<Taken<'c>, Untaken> {
    let token = IdToken::read(bearer)?;
    let held = held((config, taking), &token, now)?;
    let (_, _, entry, algorithm) = *held.first().ok_or(Untaken::Issuer)?;
    let http = issuers
        .iter()
        .find(|(issuer, _)| *issuer == entry.issuer)
        .map(|(_, http)| http);
    let kept = format!("{KEPT}{}", digest(&entry.issuer));
    let publisher = Publisher {
        kept: &kept,
        pinned: entry.keys.as_ref(),
        keys_for: entry.keys_for,
        keys_retry: entry.keys_retry,
    };
    let keys = keys(store, publisher, (token.key_id(), now), || async {
        fetched(http.ok_or(Untaken::Keys(UNREACHED.to_owned()))?, entry).await
    })
    .await?;
    let key = keys.key(token.key_id()).ok_or(Untaken::Key)?;
    let public = herald_keys::rsa_public(&key.modulus, &key.exponent);
    if !Erased(verifier)
        .verify(
            signed_with(algorithm),
            &public,
            token.signed(),
            token.signature(),
        )
        .await
    {
        return Err(Untaken::Signature);
    }
    let (app, found, entry) = matched(&token, &held)?;
    let valid = token.valid().ok_or(Untaken::Time(Untaken::NO_ISSUE))?;
    if found.replay == Replay::Once {
        match store
            .deliveries()
            .admit(TAKEN, &token.remembered_as(), now)
            .await
        {
            Ok(Admitted::Fresh) => {}
            Ok(Admitted::Seen) if again => {}
            Ok(Admitted::Seen) => return Err(Untaken::Used),
            Err(error) => return Err(Untaken::Unkept(error.to_string())),
        }
    }
    Ok(Taken {
        app,
        stands_for: &found.peers,
        entry,
        evidence: token.evidence(&entry.claims),
        valid,
        replay: found.replay,
    })
}

/// Each entry of `config` letting its tokens do `taking` and naming the
/// issuer of `token` exactly, with its app, that `token` is held to at
/// `now`: signed with an algorithm the entry allows, meant for its
/// audience and within its times.
///
/// # Errors
///
/// No entry names its issuer, or why the first naming it does not hold
/// it, an entry meant for the token's audience before any other.
fn held<'c>(
    (config, taking): (&'c Config, Taking),
    token: &IdToken,
    now: Timestamp,
) -> Result<Vec<Held<'c>>, Untaken> {
    let issuer = token.issuer().ok_or(Untaken::Issuer)?;
    let mut first = None;
    let mut held = Vec::new();
    for (app, found, entry) in config.apps.iter().flat_map(|(app, found)| {
        found
            .oidc
            .iter()
            .filter(|entry| entry.issuer == issuer && entry.takes.contains(&taking))
            .map(move |entry| (app.as_str(), found, entry))
    }) {
        let checked = token.algorithm(&entry.algorithms).and_then(|algorithm| {
            token
                .held_to(&entry.audience, now, (entry.skew, entry.lifetime))
                .map(|()| algorithm)
        });
        match checked {
            Ok(algorithm) => held.push((app, found, entry, algorithm)),
            Err(why) if first.as_ref().is_none_or(|kept| *kept == Untaken::Audience) => {
                first = Some(why);
            }
            Err(_) => {}
        }
    }
    match (held.is_empty(), first) {
        (false, _) => Ok(held),
        (true, Some(why)) => Err(why),
        (true, None) => Err(Untaken::Issuer),
    }
}

/// The first entry of `held` whose claims `token` matches, with its app.
///
/// # Errors
///
/// The first claim the first entry wants that `token` does not match.
fn matched<'c>(
    token: &IdToken,
    held: &[Held<'c>],
) -> Result<(&'c str, &'c App, &'c Oidc), Untaken> {
    let mut first = None;
    for (app, found, entry, _) in held {
        match token.matching(&entry.claims) {
            Ok(()) => return Ok((app, found, entry)),
            Err(why) => {
                first.get_or_insert(why);
            }
        }
    }
    Err(first.unwrap_or(Untaken::Issuer))
}

/// What checks a signature made with `algorithm`.
pub(crate) const fn signed_with(algorithm: TokenAlgorithm) -> Algorithm {
    match algorithm {
        TokenAlgorithm::Rs256 => Algorithm::RsaSha256,
    }
}

/// The key set of `entry`'s issuer and its body as read through `http`
/// from its `jwks`, or from the address its discovery document names on
/// its own origin.
///
/// # Errors
///
/// An answer is not 200, too large, or names keys elsewhere, or the key
/// set holds no key herald checks.
async fn fetched<H: Http>(http: &H, entry: &Oidc) -> Result<(KeySet, Vec<u8>), Untaken> {
    let address = match &entry.jwks {
        Some(jwks) => jwks.clone(),
        None => jwks_uri(
            &got(http, &discovery_of(&entry.issuer), KEYS_MOST).await?,
            &entry.issuer,
        )?,
    };
    let body = got(http, &address, KEYS_MOST).await?;
    let keys = KeySet::read(&body)?;
    if keys.is_empty() {
        return Err(Untaken::Keys(format!(
            "{address} lists no key herald checks"
        )));
    }
    Ok((keys, body))
}

/// Each issuer whose ID tokens an app of `config` takes and whose keys are
/// read, with the addresses they are read from.
pub(crate) fn issuers(config: &Config) -> Vec<(String, Vec<String>)> {
    let mut issuers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in config.apps.values().flat_map(|app| &app.oidc) {
        let bases = entry.bases();
        if !bases.is_empty() {
            issuers.entry(entry.issuer.clone()).or_insert(bases);
        }
    }
    issuers.into_iter().collect()
}

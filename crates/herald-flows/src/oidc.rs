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
use herald_core::media::JSON;
use herald_core::{
    IdToken, KeptKeys, KeySet, TokenAlgorithm, Untaken, digest, discovery_of, jwks_uri,
};
use herald_ports::{
    Admitted, Algorithm, Deliveries, DynVerifier, Erased, Http, Method, Request, SecretText, Store,
    Tokens, Verifier,
};
use jiff::Timestamp;

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

/// Why an issuer's keys older than `keys_for` are not used where they were
/// fetched, or tried, within `keys_retry`.
const UNTRIED: &str =
    "the kept ones are older than keys_for, and they were fetched or tried within keys_retry";

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
    let keys = keys((store, http), entry, token.key_id(), now).await?;
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
const fn signed_with(algorithm: TokenAlgorithm) -> Algorithm {
    match algorithm {
        TokenAlgorithm::Rs256 => Algorithm::RsaSha256,
    }
}

/// The keys of `entry`'s issuer at `now`, for a token naming `key_id`:
/// those pinned; else those `store` keeps while younger than `keys_for`
/// where they hold `key_id`; else fetched through `http` where they were
/// last fetched or tried longer than `keys_retry` ago, and kept. A fetch
/// that fails leaves the kept keys in use until they are `keys_for` old,
/// and never longer. Keys kept as read later than `now` are not fresh, and
/// a try kept as later than `now` is due, so a clock set back neither
/// keeps old keys nor holds a fetch off.
///
/// # Errors
///
/// They are older than `keys_for` and cannot be fetched now, or cannot be
/// kept.
async fn keys<S: Store, H: Http>(
    (store, http): (&S, Option<&H>),
    entry: &Oidc,
    key_id: Option<&str>,
    now: Timestamp,
) -> Result<KeySet, Untaken> {
    if let Some(pinned) = &entry.keys {
        return Ok(pinned.clone());
    }
    let tokens = store.tokens();
    let name = format!("{KEPT}{}", digest(&entry.issuer));
    let record = tokens.token(&name).await.map_err(unkept)?;
    let version = record.as_ref().map(|record| record.version);
    let kept = record.and_then(|record| KeptKeys::read(&record.value));
    let held = kept.as_ref().map(KeptKeys::keys).unwrap_or_default();
    let fresh = kept
        .as_ref()
        .and_then(|kept| kept.fetched)
        .is_some_and(|fetched| fetched <= now && now < fetched + entry.keys_for);
    if fresh && held.key(key_id).is_some() {
        return Ok(held);
    }
    let due = kept
        .as_ref()
        .is_none_or(|kept| kept.tried > now || now >= kept.tried + entry.keys_retry);
    match (due, fresh) {
        (false, true) => return Ok(held),
        (false, false) => return Err(Untaken::Keys(UNTRIED.to_owned())),
        (true, _) => {}
    }
    let http = http.ok_or(Untaken::Keys(UNREACHED.to_owned()))?;
    let (kept, read) = match fetched(http, entry).await {
        Ok((read, body)) => (KeptKeys::new(&body, now), Ok(read)),
        Err(why) => (
            kept.map_or_else(|| KeptKeys::untried(now), |kept| kept.tried_at(now)),
            if fresh { Ok(held) } else { Err(why) },
        ),
    };
    let expires = kept.fetched.unwrap_or(now) + entry.keys_for;
    tokens
        .put_token(&kept.kept(name, expires), version)
        .await
        .map_err(unkept)?;
    read
}

/// Why an issuer's keys are not read, where the store fails: `error`.
fn unkept(error: impl std::fmt::Display) -> Untaken {
    Untaken::Keys(format!("the store could not keep them: {error}"))
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
            &got(http, &discovery_of(&entry.issuer)).await?,
            &entry.issuer,
        )?,
    };
    let body = got(http, &address).await?;
    let keys = KeySet::read(&body)?;
    if keys.is_empty() {
        return Err(Untaken::Keys(format!(
            "{address} lists no key herald checks"
        )));
    }
    Ok((keys, body))
}

/// The body `http` answers a GET of `address` with, at most
/// [`KEYS_MOST`] bytes.
///
/// # Errors
///
/// It cannot be reached, or answers anything but 200.
async fn got<H: Http>(http: &H, address: &str) -> Result<Vec<u8>, Untaken> {
    let request = Request::new(Method::Get, SecretText::plain(address))
        .header("Accept", SecretText::plain(JSON))
        .at_most(KEYS_MOST);
    let response = http
        .send(request)
        .await
        .map_err(|error| Untaken::Keys(format!("{address} could not be read: {error}")))?;
    if response.status != herald_core::status::OK {
        return Err(Untaken::Keys(format!(
            "{address} answered {}",
            response.status
        )));
    }
    Ok(response.body)
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

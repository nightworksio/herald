//! An issuer's signing keys: pinned by the config, or read where it
//! publishes them and kept in the store until they are as old as they may
//! be used, fetched again for a key they lack at most once in a retry
//! however many tokens name one (ADR-0064, ADR-0093).

use std::future::Future;

use herald_core::media::JSON;
use herald_core::{KeptKeys, KeySet, Untaken};
use herald_ports::{Http, Method, Request, SecretText, Store, Tokens};
use jiff::{SignedDuration, Timestamp};

/// Why an issuer's keys older than they may be used are not read again
/// where they were fetched, or tried, within their retry.
const UNTRIED: &str =
    "the kept ones are older than keys_for, and they were fetched or tried within keys_retry";

/// Where an issuer's keys are kept and how long each read of them is
/// used.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Publisher<'a> {
    /// The store record they are kept under.
    pub(crate) kept: &'a str,
    /// Its keys, where the config pins them.
    pub(crate) pinned: Option<&'a KeySet>,
    /// How long they are used once read.
    pub(crate) keys_for: SignedDuration,
    /// How long after they were last fetched, or tried, a token naming a
    /// key they lack has them fetched again.
    pub(crate) keys_retry: SignedDuration,
}

/// The keys of `publisher` at `now`, for a token naming `key_id`: those
/// pinned; else those `store` keeps while younger than `keys_for` where
/// they name `key_id`; else as `fetch` reads them, where they were last
/// fetched or tried longer than `keys_retry` ago, and kept. A fetch that
/// fails leaves the kept keys in use until they are `keys_for` old, and
/// never longer. Keys kept as read later than `now` are not fresh, and a
/// try kept as later than `now` is due, so a clock set back neither keeps
/// old keys nor holds a fetch off.
///
/// # Errors
///
/// They are older than `keys_for` and cannot be fetched now, or cannot be
/// kept.
pub(crate) async fn keys<S: Store, F>(
    store: &S,
    publisher: Publisher<'_>,
    (key_id, now): (Option<&str>, Timestamp),
    fetch: impl FnOnce() -> F,
) -> Result<KeySet, Untaken>
where
    F: Future<Output = Result<(KeySet, Vec<u8>), Untaken>>,
{
    if let Some(pinned) = publisher.pinned {
        return Ok(pinned.clone());
    }
    let tokens = store.tokens();
    let record = tokens.token(publisher.kept).await.map_err(unkept)?;
    let version = record.as_ref().map(|record| record.version);
    let kept = record.and_then(|record| KeptKeys::read(&record.value));
    let held = kept.as_ref().map(KeptKeys::keys).unwrap_or_default();
    let fresh = kept
        .as_ref()
        .and_then(|kept| kept.fetched)
        .is_some_and(|fetched| fetched <= now && now < fetched + publisher.keys_for);
    if fresh && (held.key(key_id).is_some() || held.unendorsed(key_id)) {
        return Ok(held);
    }
    let due = kept
        .as_ref()
        .is_none_or(|kept| kept.tried > now || now >= kept.tried + publisher.keys_retry);
    match (due, fresh) {
        (false, true) => return Ok(held),
        (false, false) => return Err(Untaken::Keys(UNTRIED.to_owned())),
        (true, _) => {}
    }
    let (kept, read) = match fetch().await {
        Ok((read, body)) => (KeptKeys::new(&body, now), Ok(read)),
        Err(why) => (
            kept.map_or_else(|| KeptKeys::untried(now), |kept| kept.tried_at(now)),
            if fresh { Ok(held) } else { Err(why) },
        ),
    };
    let expires = kept.fetched.unwrap_or(now) + publisher.keys_for;
    tokens
        .put_token(&kept.kept(publisher.kept.to_owned(), expires), version)
        .await
        .map_err(unkept)?;
    read
}

/// Why an issuer's keys are not read, where the store fails: `error`.
fn unkept(error: impl std::fmt::Display) -> Untaken {
    Untaken::Keys(format!("the store could not keep them: {error}"))
}

/// The body `http` answers a GET of `address` with, at most `most` bytes.
///
/// # Errors
///
/// It cannot be reached, or answers anything but 200.
pub(crate) async fn got<H: Http>(http: &H, address: &str, most: usize) -> Result<Vec<u8>, Untaken> {
    let request = Request::new(Method::Get, SecretText::plain(address))
        .header("Accept", SecretText::plain(JSON))
        .at_most(most);
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

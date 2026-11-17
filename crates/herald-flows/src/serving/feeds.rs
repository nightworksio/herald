//! herald's API as peers read it (ADR-0046): each feed of herald's own in
//! pages, newest first, and every answer of the API signed by the
//! instance's newest key over its body's digest and the request it answers.

use std::iter::once;

use herald_core::{DestinationName, Escaping, Feed, media, status};
use herald_peers::digest::{CONTENT_DIGEST, content_digest};
use herald_peers::feed::{BEFORE, FEEDS, Listed, Page, tag};
use herald_peers::keys::method;
use herald_peers::message::{Component, Covered, Message};
use herald_ports::{Algorithm, Clock, Published, Signer, Store};

use super::discovery::{instance, public_keys};
use super::replies::{Answered, missing, refusal, replied, unmethodical};
use super::{Inbound, Serving};

/// The header naming a page's entity tag.
const ETAG: &str = "etag";
/// The header naming the entity tag a reader holds.
const IF_NONE_MATCH: &str = "if-none-match";
/// The header naming what a body is.
const CONTENT_TYPE: &str = "content-type";
/// What a host that is not the instance's domain has none of.
const NO_FEED: &str = "herald feed on this host";

/// The answer to `method` on `rest`, the path under herald's API, signed.
pub(super) async fn api<S: Store, C: Clock, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    rest: &str,
) -> Answered {
    let named = rest
        .strip_prefix(FEEDS)
        .map(|name| Escaping::UNRESERVED.unescape(name));
    let answered = match (inbound.method, named) {
        ("GET", Some(Some(name))) => feed(serving, inbound, &name).await,
        (_, Some(None) | None) => missing("such path"),
        (_, Some(Some(_))) => unmethodical(),
    };
    signed(serving, inbound, answered).await
}

/// The page of the feed `name` the request asks for: the newest, or those
/// before its `before`; not modified where the reader holds its tag.
async fn feed<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    name: &str,
) -> Answered {
    let destination = DestinationName::new(name);
    let fed = serving
        .connected
        .routes
        .iter()
        .filter(|(route, _)| route.config.route.destination == destination)
        .find_map(|(route, _)| route.reached.feeds());
    let (Some(instance), Some(fed)) = (instance(serving, inbound.host()), fed) else {
        return missing(NO_FEED);
    };
    let Ok(before) = cursor(inbound.query) else {
        return missing("such page");
    };
    let kept = match serving.ports.store.published().feed(&destination).await {
        Ok(kept) => kept.map(|kept| kept.value).unwrap_or_default(),
        Err(error) => return refusal(status::UNAVAILABLE, error.to_string()),
    };
    let size = fed.feeding.page;
    let etag = tag(kept.newest, before, size);
    let mut answered = if inbound.header(IF_NONE_MATCH) == Some(etag.as_str()) {
        replied(status::NOT_MODIFIED, media::JSON, Vec::new())
    } else {
        let url = instance.feed(name);
        replied(status::OK, media::JSON, page(&kept, &url, before, size))
    };
    answered.reply.headers.push((ETAG.to_owned(), etag));
    answered
}

/// The page of `feed`, at `url`, listing at most `size` envelopes before
/// `before`.
fn page(feed: &Feed, url: &str, before: Option<u64>, size: usize) -> String {
    let (items, more) = feed.page(before, size);
    Page {
        feed: url,
        before,
        listed: items
            .iter()
            .map(|item| Listed {
                sequence: item.sequence,
                envelope: &item.envelope,
            })
            .collect(),
        more,
    }
    .document()
}

/// The sequence `query` asks for envelopes before, where it names one.
///
/// # Errors
///
/// It names one that is not a number.
fn cursor(query: Option<&str>) -> Result<Option<u64>, std::num::ParseIntError> {
    query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .find_map(|pair| pair.strip_prefix(BEFORE)?.strip_prefix('='))
        .map(str::parse)
        .transpose()
}

/// `answered`, signed by the instance's newest key where it has one, over
/// its status, type, body's digest and entity tag, and the method and URL
/// of `inbound`; refused where it cannot be.
pub(super) async fn signed<S, C: Clock, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    mut answered: Answered,
) -> Answered {
    if serving.connected.instance.is_empty() {
        return answered;
    }
    let reply = &mut answered.reply;
    reply
        .headers
        .push((CONTENT_DIGEST.to_owned(), content_digest(&reply.body)));
    match signature(serving, inbound, &answered.reply).await {
        Ok(fields) => {
            answered.reply.headers.extend(fields);
            answered
        }
        Err(why) => refusal(status::SERVER_ERROR, why),
    }
}

/// The fields carrying the signature of `reply` to `inbound` by the
/// instance's newest key.
///
/// # Errors
///
/// The key is no Ed25519 key, it cannot sign, or the request names no host.
async fn signature<S, C: Clock, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    reply: &super::Reply,
) -> Result<[(String, String); 2], String> {
    let publics = public_keys(serving).await?;
    let signer = serving.ports.signer;
    let mut carried = Err("the instance has no key".to_owned());
    for (key, public) in serving.connected.instance.iter().zip(&publics).take(1) {
        let domain = serving
            .config
            .federation
            .domain
            .as_deref()
            .unwrap_or_default();
        let fields: Vec<(String, String)> = once((CONTENT_TYPE.to_owned(), reply.kind.to_owned()))
            .chain(reply.headers.iter().cloned())
            .collect();
        let now = serving.ports.clock.now().as_second();
        let lasts = serving.config.federation.signed_for.as_secs();
        let covered = Covered {
            components: covered(&reply.headers),
            created: now,
            expires: now.saturating_add(lasts),
            nonce: None,
            keyid: method(domain, public),
        };
        let query = inbound
            .query
            .map(|query| format!("?{query}"))
            .unwrap_or_default();
        let url = format!("https://{}{}{query}", inbound.host(), inbound.path);
        let message = Message {
            method: inbound.method,
            url: &url,
            status: Some(reply.status),
            fields: &fields,
        };
        let base = covered.base(&message).map_err(said)?;
        let signature = signer
            .sign(Algorithm::Ed25519, key, &base)
            .await
            .map_err(said)?;
        carried = Ok(covered.fields(&signature));
    }
    carried
}

/// What `error` says.
fn said(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// What a signature of a reply carrying `headers` covers: its status, type
/// and body's digest, its entity tag where it has one, and the method and
/// URL of the request it answers.
fn covered(headers: &[(String, String)]) -> Vec<Component> {
    let field = |name: &str| Component::Field(name.to_owned());
    let tagged = headers
        .iter()
        .any(|(name, _)| name == ETAG)
        .then(|| field(ETAG));
    [
        Component::Status,
        field(CONTENT_TYPE),
        field(CONTENT_DIGEST),
    ]
    .into_iter()
    .chain(tagged)
    .chain([
        Component::Request(Box::new(Component::Method)),
        Component::Request(Box::new(Component::TargetUri)),
    ])
    .collect()
}

//! The instance's feeds written as a static signed log for any static host
//! to serve (ADR-0060): each feed's pages, and the discovery document
//! listing the digest of each under the log's next sequence, signed beside
//! it by the instance's newest key, written where a feed or the key set
//! changed or the log was last written `refresh` ago, to a branch in one
//! commit and handed back whole for a directory.

use std::collections::BTreeMap;
use std::fmt;

use herald_config::Config;
use herald_core::{DestinationName, Publication, itself};
use herald_keys::ED25519_LENGTH;
use herald_peers::digest::content_digest;
use herald_peers::discovery::Instance;
use herald_peers::feed::Envelope;
use herald_peers::keys::{KeySet, method};
use herald_peers::multibase::ed25519_public;
use herald_peers::pages::{Log, Logged, NOJEKYLL, Written, signature_of, statement};
use herald_peers::proof;
use herald_ports::{Algorithm, Clock, Http, Published, Secret, Signer, Store, Version};
use jiff::Timestamp;

use crate::announce::{Ports, Stop};
use crate::prepare::{Connected, PreparedPages};
use crate::serving::discovery::{proved_set, publics, published};

mod branch;

/// What a writing of the static signed log did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publishing {
    /// No feed nor the key set changed, and it was written less than
    /// `refresh` ago: it stands at this sequence.
    Current(u64),
    /// It was written under `sequence`.
    Written {
        /// Its sequence.
        sequence: u64,
        /// Every file it holds, each by its path under the log's URL.
        files: Vec<(String, String)>,
        /// How many files the branch's commit wrote or removed; none where
        /// it is committed to no branch.
        changed: usize,
    },
}

/// The feeds rendered: the key set proved, each feed's pages, and the
/// digest of each file the log holds that changes only where they do.
struct Rendered {
    keys: Envelope,
    feeds: Vec<(String, u64, Vec<Written>)>,
    digests: BTreeMap<String, String>,
}

/// The static signed log `[federation.pages]` names, written where it is
/// due; nothing where the config names none.
///
/// # Errors
///
/// The store, a key or the branch could not be read or written, or
/// another run wrote the record or the branch first each time.
pub async fn publish<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
) -> Result<Option<Publishing>, Stop> {
    let (Some((pages, http)), Some(secret)) = (&connected.pages, connected.instance.first()) else {
        return Ok(None);
    };
    let now = ports.clock.now();
    let instance = instance(config, pages);
    let publics = publics(ports.signer, &connected.instance)
        .await
        .map_err(Stop::Proof)?;
    let rendered = rendered((config, connected, ports), &instance, &publics, now).await?;
    let kept = ports.store.published();
    let attempts = config.attempts.get();
    for _ in 0..attempts {
        let (held, version) = match kept.publication().await.map_err(stored)? {
            Some(held) => (held.value, Some(held.version)),
            None => (Publication::default(), None),
        };
        if !held.due(&rendered.digests, now, pages.pages.refresh) {
            return Ok(Some(Publishing::Current(held.sequence)));
        }
        let begun = held.begun();
        if let herald_ports::Written::Written(at) = kept
            .put_publication(&begun, version)
            .await
            .map_err(stored)?
        {
            let log = Log {
                sequence: begun.sequence,
                published: now,
            };
            let signing = publics
                .first()
                .map(|public| method(instance.domain, public))
                .unwrap_or_default();
            let files = files(
                &instance,
                &rendered,
                ((ports.signer, secret, &signing), log),
            )
            .await?;
            let (commit, changed) = match &pages.pages.branch {
                Some(name) => {
                    let wrote = branch::committed(http, (pages, name), &files, &held, log).await?;
                    (Some(wrote.0), wrote.1)
                }
                None => (None, 0),
            };
            let record = Publication {
                sequence: begun.sequence,
                published: Some(now),
                files: rendered.digests,
                commit,
            };
            recorded(kept, &record, at).await?;
            return Ok(Some(Publishing::Written {
                sequence: begun.sequence,
                files,
                changed,
            }));
        }
    }
    Err(Stop::Store(format!(
        "the static log's record was written by another run each of {attempts} times herald wrote it"
    )))
}

/// The instance as the static log names it, served under its URL.
fn instance<'a>(config: &'a Config, pages: &'a PreparedPages) -> Instance<'a> {
    Instance {
        domain: config.federation.domain.as_deref().unwrap_or_default(),
        base: &pages.pages.base,
        software: itself::NAME,
        version: itself::VERSION,
        repository: herald_core::repository!(),
    }
}

/// Each feed a route publishes to rendered as pages, and the key set of
/// `publics` proved at `now`, with the digest of each file.
async fn rendered<S: Store, C, G: Signer, H>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    instance: &Instance<'_>,
    publics: &[[u8; ED25519_LENGTH]],
    now: Timestamp,
) -> Result<Rendered, Stop> {
    let written = publics.iter().map(ed25519_public).collect();
    let sequence = published(ports.store, written, config.attempts.get())
        .await
        .map_err(stored)?;
    let set = KeySet {
        sequence,
        refresh: config.federation.refresh.as_secs().unsigned_abs(),
        keys: publics.to_vec(),
    };
    let mut sized: BTreeMap<&str, usize> = BTreeMap::new();
    for (route, _) in &connected.routes {
        if let Some(fed) = route.reached.feeds() {
            sized
                .entry(route.config.route.destination.as_str())
                .or_insert(fed.feeding.page);
        }
    }
    let kept = ports.store.published();
    let mut feeds = Vec::new();
    for (name, size) in sized {
        let feed = kept
            .feed(&DestinationName::new(name))
            .await
            .map_err(stored)?
            .map(|kept| kept.value)
            .unwrap_or_default();
        let pages = instance.pages(name, &feed, size);
        feeds.push((name.to_owned(), feed.newest, pages));
    }
    let unproved = set.document(instance.domain);
    let placed = Log {
        sequence: 0,
        published: Timestamp::UNIX_EPOCH,
    };
    let mut digests = BTreeMap::new();
    digests.insert(
        under(instance, &instance.discovery()).to_owned(),
        content_digest(
            instance
                .logged(&unproved, &logged(&feeds), placed)
                .as_bytes(),
        ),
    );
    for (_, _, pages) in &feeds {
        for page in pages {
            digests.insert(
                under(instance, &page.url).to_owned(),
                content_digest(page.body.as_bytes()),
            );
        }
    }
    let keys = proved_set(
        ports.signer,
        (instance.domain, now),
        &set,
        connected.instance.last(),
    )
    .await;
    Ok(Rendered {
        keys,
        feeds,
        digests,
    })
}

/// `feeds` as the discovery document lists them.
fn logged(feeds: &[(String, u64, Vec<Written>)]) -> Vec<Logged<'_>> {
    feeds
        .iter()
        .map(|(name, newest, pages)| Logged {
            name,
            newest: *newest,
            pages,
        })
        .collect()
}

/// Every file of the log written under `log`, each by its path: each page,
/// the discovery document, its signature by the instance's newest key,
/// `secret`, which the verification method `by` names, and the file telling GitHub Pages to
/// serve `.well-known`.
async fn files<G: Signer>(
    instance: &Instance<'_>,
    rendered: &Rendered,
    ((signer, secret, by), log): ((&G, &Secret, &str), Log),
) -> Result<Vec<(String, String)>, Stop> {
    let url = instance.discovery();
    let discovery = instance.logged(&rendered.keys, &logged(&rendered.feeds), log);
    let unproved = proof::unproved(
        &statement(&url, discovery.as_bytes(), log),
        by,
        log.published,
    )
    .map_err(proving)?;
    let signature = signer
        .sign(Algorithm::Ed25519, secret, unproved.message())
        .await
        .map_err(proving)?;
    let mut files: Vec<(String, String)> = rendered
        .feeds
        .iter()
        .flat_map(|(_, _, pages)| pages)
        .map(|page| (under(instance, &page.url).to_owned(), page.body.clone()))
        .collect();
    files.push((
        under(instance, &signature_of(&url)).to_owned(),
        unproved.proved(&signature).to_string(),
    ));
    files.push((under(instance, &url).to_owned(), discovery));
    files.push((NOJEKYLL.to_owned(), String::new()));
    Ok(files)
}

/// The path of `url` under the log's URL.
fn under<'u>(instance: &Instance<'_>, url: &'u str) -> &'u str {
    let root = format!("https://{}{}/", instance.domain, instance.base);
    url.strip_prefix(&root).unwrap_or(url)
}

/// `record` written over the record its writing began at, `at`; left
/// where another writing began meanwhile, as only one does, and it writes
/// the log again, every file where the branch no longer stands where the
/// record says.
async fn recorded<P: Published>(kept: P, record: &Publication, at: Version) -> Result<(), Stop> {
    kept.put_publication(record, Some(at))
        .await
        .map(drop)
        .map_err(stored)
}

/// Why the store stopped the writing.
fn stored(error: impl fmt::Display) -> Stop {
    Stop::Store(error.to_string())
}

/// Why the log could not be signed.
fn proving(error: impl fmt::Display) -> Stop {
    Stop::Proof(format!("the static log could not be signed: {error}"))
}

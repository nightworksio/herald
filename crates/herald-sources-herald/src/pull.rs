use std::collections::BTreeMap;
use std::fmt;

use herald_core::{
    Attribute, Cursor, Draft, Event, Identity, Kind, KindSpecs, Origin, Polled, Proof, Tombstone,
    Unheld, Visibility, media, status,
};
use herald_kinds::{Peering, PublicKey, Pull, Pulling, Trust, Unpulled};
use herald_peers::answer::{Answer, Bounds, Unanswered, checked, entity_tag, unchanged};
use herald_peers::feed::{BEFORE, PAGE_MOST};
use herald_peers::keys::{WELL_KNOWN, method};
use herald_peers::proof;
use herald_peers::reading::{self, Carried, Enveloped};
use herald_ports::{
    Algorithm, Boxed, Erased, Http as _, Method, Request, Response, SecretText, Verifier as _,
};
use jiff::Timestamp;
use serde_json::Value;

use crate::options::{HeraldOptions, NAMESPACE, Verify};

/// The attribute naming the forge a peer's event claims to come from, where
/// the source does not grant its identity.
pub const CLAIMED_FORGE: &str = "claimed_forge";
/// The attribute naming the host a peer's event claims to come from.
pub const CLAIMED_HOST: &str = "claimed_host";
/// The attribute naming the repository a peer's event claims to come from.
pub const CLAIMED_REPOSITORY: &str = "claimed_repository";

/// A peer's feed, pulled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pulled(HeraldOptions);

impl Pulled {
    pub(crate) fn new(options: &HeraldOptions) -> Self {
        Self(options.clone())
    }
}

impl Pull for Pulled {
    fn api(&self) -> &str {
        &self.0.api
    }

    fn peering(&self) -> Peering<'_> {
        self.0.reads()
    }

    fn remembered(&self) -> usize {
        self.0.pages.saturating_mul(PAGE_MOST)
    }

    fn pulled<'a>(
        &'a self,
        pulling: Pulling<'a>,
        trust: &'a Trust,
        cursor: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpulled>> {
        Box::pin(self.read(pulling, trust, cursor))
    }
}

impl Pulled {
    /// What the feed lists since `cursor`, newest first: every page down to
    /// the cursor's sequence, at most `pages` of them, or with no cursor
    /// enough for `backfill`.
    async fn read(
        &self,
        pulling: Pulling<'_>,
        trust: &Trust,
        cursor: Option<&Cursor>,
    ) -> Result<Option<Polled>, Unpulled> {
        let since = cursor.and_then(|cursor| cursor.sequence);
        let mut polled = Polled {
            relisted: true,
            ..Polled::default()
        };
        let mut shares = Shares::of(cursor, self.0.peer_share);
        let mut before: Option<u64> = None;
        for _ in 0..self.0.pages {
            let url = before.map_or_else(
                || self.0.api.clone(),
                |before| format!("{}?{BEFORE}={before}", self.0.api),
            );
            let etag = cursor
                .filter(|_| before.is_none())
                .and_then(|cursor| cursor.etag.as_deref());
            let Some((answer, tag)) = fetched(pulling, trust, &url, etag).await? else {
                return Ok(None);
            };
            if before.is_none() {
                polled.etag = tag;
            }
            let page = reading::page(&answer.body, &url).map_err(unreadable)?;
            let mut reached = !page.more;
            for (sequence, envelope) in &page.items {
                if since.is_some_and(|since| *sequence <= since) {
                    reached = true;
                    break;
                }
                polled.sequence.get_or_insert(*sequence);
                self.taken(
                    (pulling, trust, cursor),
                    (*sequence, envelope),
                    (&mut polled, &mut shares),
                )
                .await?;
                before = Some(*sequence);
            }
            let found = polled.events.len() + polled.passed.len();
            if reached || (cursor.is_none() && found >= self.0.backfill) {
                break;
            }
        }
        let makers = match self.0.verify {
            Verify::Hop => trust.origins.len().saturating_add(1),
            Verify::Origin => 1,
        };
        polled.each = Some((self.remembered() / makers).max(1));
        polled.unheld = Unheld {
            most: self.0.tombstones,
            each: self.0.peer_tombstones,
        };
        if let Some(capped) = shares.capped {
            polled.sequence = polled.sequence.map(|sequence| sequence.min(capped));
            polled.etag = None;
        }
        polled.refused = shares.refused;
        polled.watched = polled.events.len();
        Ok(Some(polled))
    }

    /// `envelope` taken into `polled`, once its proof holds for the
    /// instance that made it and is newer than any `cursor` or `polled`
    /// took of its identity: its event, or the identity it withdraws where
    /// `cursor` took an event of it, else only its proof, as one a later
    /// tombstone pushes out; or
    /// its identity passed where the event is of no kind the config
    /// declares or the envelope came by a path the source does not take;
    /// nothing where its proof was made too far ahead, the instance that
    /// made it named in `polled.ahead`, or where it would take a new
    /// identity of an instance `shares` holds as many of as it may, read
    /// again from its place in the feed, `sequence`.
    async fn taken(
        &self,
        (pulling, trust, cursor): (Pulling<'_>, &Trust, Option<&Cursor>),
        (sequence, envelope): (u64, &Value),
        (polled, shares): (&mut Polled, &mut Shares),
    ) -> Result<(), Unpulled> {
        let carried = reading::carried(envelope).map_err(unreadable)?;
        let (identity, path) = match &carried {
            Carried::Event(event) => (&event.identity, &event.path),
            Carried::Withdrawn { identity, path } => (identity, path),
        };
        let Some(maker) = self.made_by(trust, path) else {
            polled
                .passed
                .push(self.namespaced(identity, &self.0.name, false).1);
            return Ok(());
        };
        let (granted, identity) = self.namespaced(identity, maker.name, maker.peer);
        let Some(created) =
            proved((pulling, trust.skew), maker.domain, maker.keys, envelope).await?
        else {
            named(&mut polled.ahead, maker.name);
            return Ok(());
        };
        let newer = !polled.proved.iter().any(|proof| proof.identity == identity)
            && cursor.is_none_or(|cursor| cursor.newer(&identity, created));
        if !newer {
            return Ok(());
        }
        let held = cursor.is_some_and(|cursor| cursor.took(&identity));
        if matches!(carried, Carried::Event(_))
            && !shares.admits(
                maker.name,
                cursor.and_then(|cursor| cursor.proof(&identity)),
            )
        {
            shares.refuse(maker.name, sequence);
            return Ok(());
        }
        let (event, tombstone) = match carried {
            Carried::Event(event) => (
                self.event(*event, (identity.clone(), granted), &maker, pulling.specs),
                None,
            ),
            Carried::Withdrawn { .. } if held => {
                if !cursor
                    .and_then(|cursor| cursor.proof(&identity))
                    .is_some_and(Proof::held)
                {
                    shares.hold(maker.name);
                }
                (None, Some(Tombstone::Held))
            }
            Carried::Withdrawn { .. } => (None, Some(Tombstone::Unheld)),
        };
        polled.proved.push(Proof {
            identity: identity.clone(),
            created,
            maker: maker.name.to_owned(),
            tombstone: tombstone.clone(),
            passed: event.is_none() && tombstone.is_none(),
        });
        match (event, tombstone) {
            (Some(event), _) => polled.events.push(event),
            (None, None) => polled.passed.push(identity),
            (None, Some(Tombstone::Held)) => polled.withdrawn.push(identity),
            (None, Some(Tombstone::Unheld)) => {}
        }
        Ok(())
    }

    /// The namespace the source grants `identity`, made by the instance
    /// `maker` names, under, and the identity it is taken as: itself where
    /// granted, else under `herald/<maker>/`. Only an identity the pulled
    /// peer made itself, `peer`, is ever granted.
    fn namespaced<'s>(
        &'s self,
        identity: &str,
        maker: &str,
        peer: bool,
    ) -> (Option<&'s str>, Identity) {
        let granted = self
            .0
            .namespaces
            .iter()
            .find(|namespace| {
                peer && identity
                    .strip_prefix(namespace.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            .map(String::as_str);
        let taken = match granted {
            Some(_) => identity.to_owned(),
            None => format!("{NAMESPACE}/{maker}/{identity}"),
        };
        (granted, Identity::new(taken))
    }

    /// The domain and keys of the instance that made an envelope that came
    /// by `path`, where the source takes that path: the peer alone, or with
    /// `verify = "hop"`, a peer of `[peers]` whose envelope the peer passed
    /// on through at most its `hops` instances, none twice.
    fn made_by<'t>(&'t self, trust: &'t Trust, path: &[String]) -> Option<Maker<'t>> {
        let peer = format!("{}{WELL_KNOWN}", trust.url);
        if let [only] = path
            && *only == peer
        {
            return Some(Maker {
                name: &self.0.name,
                domain: &trust.domain,
                keys: &trust.keys,
                peer: true,
            });
        }
        let hops = usize::from(trust.hops);
        let distinct = path
            .iter()
            .enumerate()
            .all(|(at, hop)| !path.iter().take(at).any(|before| before == hop));
        if self.0.verify != Verify::Hop
            || path.len() > hops
            || !distinct
            || path.last() != Some(&peer)
        {
            return None;
        }
        let first = path.first()?;
        trust
            .origins
            .iter()
            .find(|origin| format!("{}{WELL_KNOWN}", origin.url) == *first)
            .map(|origin| Maker {
                name: &origin.name,
                domain: &origin.domain,
                keys: &origin.keys,
                peer: false,
            })
    }

    /// The event `enveloped` carries, as `identity`, where it is of a kind
    /// `specs` declares with the attributes that kind takes: its origin
    /// the instance `maker` names, or the one it names where that too is
    /// under the namespace `granted` its identity is under.
    fn event(
        &self,
        enveloped: Enveloped,
        (identity, granted): (Identity, Option<&str>),
        maker: &Maker<'_>,
        specs: &KindSpecs,
    ) -> Option<Event> {
        let kind = Kind::new(&enveloped.kind).ok().filter(|kind| !kind.own())?;
        let spec = specs.get(&kind)?;
        let mut attributes: BTreeMap<String, Attribute> = enveloped
            .attributes
            .into_iter()
            .filter(|(name, _)| spec.attributes.contains_key(name))
            .map(|(name, value)| Some((name, serde_json::from_value(value).ok()?)))
            .collect::<Option<_>>()?;
        spec.check(&attributes).ok()?;
        let named = enveloped
            .origin
            .as_ref()
            .filter(|[_, host, repository]| {
                granted.is_some_and(|namespace| under(namespace, &format!("{host}/{repository}")))
            })
            .and_then(|[_, host, repository]| self.claimed(host, repository));
        if let Some([forge, host, repository]) = enveloped.origin {
            attributes.insert(CLAIMED_FORGE.to_owned(), Attribute::Text(forge));
            attributes.insert(CLAIMED_HOST.to_owned(), Attribute::Text(host));
            attributes.insert(CLAIMED_REPOSITORY.to_owned(), Attribute::Text(repository));
        }
        let origin = named.unwrap_or_else(|| made(maker));
        let draft = Draft {
            identity: identity.as_str().to_owned(),
            kind,
            project: enveloped.project,
            title: enveloped.title,
            body: enveloped.body,
            links: enveloped.links,
            attributes,
            occurred: enveloped.occurred,
        };
        Event::normalise(draft)
            .ok()
            .map(|event| event.from_origin(origin))
    }

    /// The origin an event whose identity the source grants names: its
    /// host and repository, where they are ones a source could give, its
    /// forge `herald`, as no envelope proves which forge it names.
    fn claimed(&self, host: &str, repository: &str) -> Option<Origin> {
        let owner = repository
            .rsplit_once('/')
            .map(|(owner, _)| owner.to_owned())
            .unwrap_or_default();
        Origin {
            id: format!("{NAMESPACE}/{}/{host}/{repository}", self.0.name),
            forge: NAMESPACE.to_owned(),
            host: host.to_owned(),
            owner,
            repository: repository.to_owned(),
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility: Visibility::Public,
            source: None,
        }
        .checked()
        .ok()
    }
}

/// How many tombstones of identities its source took a pull holds of each
/// instance, those its cursor holds counted; the instances it took no more
/// new identities of, and the place in the feed just before the first
/// envelope it took no more of.
#[derive(Debug)]
struct Shares {
    held: BTreeMap<String, usize>,
    share: usize,
    refused: Vec<String>,
    capped: Option<u64>,
}

impl Shares {
    /// The shares of a pull after `cursor`, which takes no new identity of
    /// an instance whose tombstones it holds `share` of.
    fn of(cursor: Option<&Cursor>, share: usize) -> Self {
        Self {
            held: cursor
                .map(|cursor| {
                    cursor
                        .held()
                        .into_iter()
                        .map(|(maker, held)| (maker.to_owned(), held))
                        .collect()
                })
                .unwrap_or_default(),
            share,
            refused: Vec::new(),
            capped: None,
        }
    }

    /// Whether `maker` may have the pull take an envelope of an identity
    /// whose proof the cursor keeps as `kept`: always where it holds the
    /// identity already, else while it holds fewer than `share` of its
    /// tombstones.
    fn admits(&self, maker: &str, kept: Option<&Proof>) -> bool {
        kept.is_some_and(|kept| kept.tombstone != Some(Tombstone::Unheld))
            || self.held.get(maker).copied().unwrap_or_default() < self.share
    }

    /// `maker` holds one more tombstone of an identity its source took.
    fn hold(&mut self, maker: &str) {
        *self.held.entry(maker.to_owned()).or_default() += 1;
    }

    /// `maker` refused an envelope at `sequence`, which a later pull reads
    /// again.
    fn refuse(&mut self, maker: &str, sequence: u64) {
        named(&mut self.refused, maker);
        let before = sequence.saturating_sub(1);
        self.capped = Some(self.capped.map_or(before, |capped| capped.min(before)));
    }
}

/// `names` with `name`, once.
fn named(names: &mut Vec<String>, name: &str) {
    if !names.iter().any(|named| named == name) {
        names.push(name.to_owned());
    }
}

/// The instance that made an envelope: its name in identities, its domain,
/// the keys that stand for it, and whether it is the pulled peer itself.
#[derive(Debug, Clone, Copy)]
struct Maker<'t> {
    name: &'t str,
    domain: &'t str,
    keys: &'t [PublicKey],
    peer: bool,
}

/// The origin of every event `maker` made whose identity the source does
/// not grant: the instance itself, as a `herald` forge, never the forge
/// the event claims.
fn made(maker: &Maker<'_>) -> Origin {
    Origin {
        forge: NAMESPACE.to_owned(),
        host: maker.domain.to_owned(),
        owner: String::new(),
        repository: maker.name.to_owned(),
        id: format!("{NAMESPACE}/{}", maker.name),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        source: None,
    }
}

/// The answer to a GET of `url`, checked against `trust` before its body
/// is read, asked only for a change from `etag` where it is given, and
/// the entity tag its signature covers; `None` where an answer signed
/// over `etag` says nothing changed since.
async fn fetched(
    pulling: Pulling<'_>,
    trust: &Trust,
    url: &str,
    etag: Option<&str>,
) -> Result<Option<(Response, Option<String>)>, Unpulled> {
    let request = Request::new(Method::Get, SecretText::plain(url))
        .accepting(media::JSON)
        .at_most(trust.most)
        .identified()
        .conditional(etag, None);
    let response = Erased(pulling.http)
        .send(request)
        .await
        .map_err(|error| Unpulled::Unreachable(error.to_string()))?;
    let unmodified = etag.filter(|_| response.status == status::NOT_MODIFIED);
    if unmodified.is_none() && response.status != status::OK {
        return Err(Unpulled::Unreadable(format!(
            "the feed was answered {}",
            response.status
        )));
    }
    let answer = Answer {
        method: Method::Get.as_str(),
        url,
        status: response.status,
        fields: &response.headers,
        body: &response.body,
    };
    let bounds = Bounds {
        fields: trust.fields,
        now: pulling.now.as_second(),
        skew: trust.skew,
        longest: trust.longest,
    };
    let (by, shown) = match unmodified {
        Some(etag) => unchanged(&answer, &trust.domain, &trust.keys, bounds, etag),
        None => checked(&answer, &trust.domain, &trust.keys, bounds),
    }
    .map_err(refused)?;
    if !Erased(pulling.verifier)
        .verify(Algorithm::Ed25519, by, &shown.base, &shown.signature)
        .await
    {
        return Err(Unpulled::Unreadable(
            "the feed's answer's signature does not hold".to_owned(),
        ));
    }
    let tag = entity_tag(&answer, &shown);
    Ok(unmodified.is_none().then_some((response, tag)))
}

/// When `envelope`'s proof was made, once it holds for one of `keys`, of
/// the instance on `domain`; nothing where it was made more than `skew`
/// seconds ahead of the round, which no proof may be.
async fn proved(
    (pulling, skew): (Pulling<'_>, i64),
    domain: &str,
    keys: &[PublicKey],
    envelope: &Value,
) -> Result<Option<Timestamp>, Unpulled> {
    let shown = proof::shown(envelope).map_err(unreadable)?;
    let key = keys
        .iter()
        .find(|key| method(domain, key) == shown.method)
        .ok_or_else(|| {
            Unpulled::Unreadable(format!(
                "an envelope is proved by {}, a key not trusted for {domain}",
                shown.method
            ))
        })?;
    if !Erased(pulling.verifier)
        .verify(Algorithm::Ed25519, key, &shown.message, &shown.signature)
        .await
    {
        return Err(Unpulled::Unreadable(
            "an envelope's proof does not hold".to_owned(),
        ));
    }
    let created: Timestamp = shown.created.parse().map_err(|_| {
        Unpulled::Unreadable(format!(
            "an envelope's proof was made at {}, which is no time",
            shown.created
        ))
    })?;
    Ok((created.as_second() <= pulling.now.as_second().saturating_add(skew)).then_some(created))
}

/// Whether `path` is `namespace` or lies under it, on a `/` boundary.
fn under(namespace: &str, path: &str) -> bool {
    path.strip_prefix(namespace)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Why a peer's answer is refused, as a pull reading nothing.
fn refused(why: Unanswered) -> Unpulled {
    match why {
        Unanswered::Stranger(keyid) => Unpulled::Stranger(keyid),
        other => unreadable(other),
    }
}

/// What could not be read, as `why` says.
fn unreadable(why: impl fmt::Display) -> Unpulled {
    Unpulled::Unreadable(why.to_string())
}

#[cfg(test)]
mod tests;

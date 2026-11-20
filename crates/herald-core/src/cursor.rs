use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::key::{Identity, digest};
use crate::signal::{Signal, Validators};

/// What a polled source has seen, and the validators its server last returned.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cursor {
    /// The identities seen, the newest first.
    pub seen: Vec<Identity>,
    /// The `ETag` the server last returned.
    pub etag: Option<String>,
    /// The `Last-Modified` the server last returned.
    pub last_modified: Option<String>,
    /// The project its newest event was announced as.
    #[serde(default)]
    pub project: Option<String>,
    /// The validators the server last returned for each signal asked apart
    /// from the releases.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub signals: BTreeMap<Signal, Validators>,
    /// The identities of `seen` it has seen as a tag and not yet as a
    /// Release, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tagged: Vec<Identity>,
    /// A digest of the name and notes of each Release it watches for an
    /// edit (ADR-0034).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<Identity, String>,
    /// The validators the server last returned for each package's
    /// versions, by package (ADR-0035).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packages: BTreeMap<String, Validators>,
    /// The validators the server last returned for each further listing a
    /// signal reads, by the listing's name, such as a run's `check-suite`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub listings: BTreeMap<String, Validators>,
    /// When the newest event it announced occurred, where its source
    /// leaves older ones it finds unannounced (ADR-0042).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest: Option<Timestamp>,
    /// Each instance whose envelope the last pull passed over, as its proof
    /// was made further ahead of the round than any may be (ADR-0049).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ahead: Vec<String>,
    /// The newest sequence of another herald's feed it has read
    /// (ADR-0049).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// The sequence of the newest static signed log of another herald it
    /// has read, so it takes no older one again (ADR-0060).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<u64>,
    /// When the proof of the newest envelope it took of each identity from
    /// another herald's feed was made, the latest taken first (ADR-0049).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proved: Vec<Proof>,
    /// The project of each event it took from another herald's feed and
    /// still sees (ADR-0049).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<Identity, String>,
    /// Each living post of a report a route was sent its advisories
    /// section in, until herald edits the section out (ADR-0067).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advised: Vec<crate::AdvisedPost>,
    /// Each route of a report sent its view's advisories section whose post
    /// of the window it remembers stopped: sent again alone, so no other
    /// route of the report is sent anew for it (ADR-0067).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<crate::RouteId>,
}

/// When the proof of the newest envelope a source took of one identity from
/// another herald's feed was made (ADR-0049).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    /// The identity.
    pub identity: Identity,
    /// When the proof was made.
    pub created: Timestamp,
    /// The instance that made it.
    #[serde(default)]
    pub maker: String,
    /// What the envelope withdrew, where it was a tombstone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tombstone: Option<Tombstone>,
    /// Whether the source passed the envelope's event over.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub passed: bool,
}

/// What a tombstone a source took withdrew (ADR-0049).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tombstone {
    /// An identity the source took, whose proof it never forgets.
    Held,
    /// An identity the source never took, whose proof later tombstones push
    /// out.
    Unheld,
}

/// How many tombstones of identities its source never took a cursor
/// keeps: in all, and of any one instance (ADR-0049).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Unheld {
    /// The most in all.
    pub most: usize,
    /// The most of one instance.
    pub each: usize,
}

/// What one poll of a source found: its events, the newest first, and the
/// validators the server returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Polled {
    /// The events, the newest first.
    pub events: Vec<Event>,
    /// The `ETag` the server returned.
    pub etag: Option<String>,
    /// The `Last-Modified` the server returned.
    pub last_modified: Option<String>,
    /// The validators the server returned for each signal asked apart from
    /// the releases.
    pub signals: BTreeMap<Signal, Validators>,
    /// The identities it found without reading them in full, newest first,
    /// which announce nothing and are remembered as seen.
    pub passed: Vec<Identity>,
    /// How many of the newest Releases it found are watched for an edit.
    pub watched: usize,
    /// The validators the server returned for each package's versions, by
    /// package.
    pub packages: BTreeMap<String, Validators>,
    /// The identities of package versions it found without reading them in
    /// full, newest first, which announce nothing and are remembered as
    /// seen.
    pub unread: Vec<Identity>,
    /// The validators the server returned for each further listing a
    /// signal reads, by the listing's name.
    pub listings: BTreeMap<String, Validators>,
    /// When the newest event its source announced occurred, where the
    /// source keeps that mark; the cursor's own mark stays where it is
    /// not given.
    pub newest: Option<Timestamp>,
    /// The newest sequence of another herald's feed it read, where it read
    /// one; the cursor's own stays where it is not given.
    pub sequence: Option<u64>,
    /// The sequence of the static signed log it read, where it read one;
    /// the cursor's own stays where it is not given.
    pub log: Option<u64>,
    /// The identities another herald's feed withdrew by a tombstone, newest
    /// first, which are no longer seen.
    pub withdrawn: Vec<Identity>,
    /// When the proof of each envelope it took from another herald's feed
    /// was made, the latest taken first, once each.
    pub proved: Vec<Proof>,
    /// How many tombstones of identities its source never took the cursor
    /// keeps after it.
    pub unheld: Unheld,
    /// How many proofs of envelopes that are no tombstones the cursor keeps
    /// of each instance after it, where it keeps them by instance; else
    /// `remembered` of all.
    pub each: Option<usize>,
    /// Each instance it took no more new identities of, as it holds as
    /// many of its identities as one instance may have it hold.
    pub refused: Vec<String>,
    /// Each instance it passed an envelope of over, as its proof was made
    /// further ahead of the round than any may be.
    pub ahead: Vec<String>,
    /// Whether its source lists only what changed since the cursor, an
    /// edited event entering again at the top, so the digests watched
    /// before stay beside its own for every identity still seen.
    pub relisted: bool,
}

impl Polled {
    /// The identities it took an envelope of.
    fn taken(&self) -> BTreeSet<&Identity> {
        self.proved.iter().map(|proof| &proof.identity).collect()
    }
}

impl Proof {
    /// Whether it is the proof of a tombstone of an identity its source
    /// took.
    #[must_use]
    pub fn held(&self) -> bool {
        self.tombstone == Some(Tombstone::Held)
    }
}

impl Cursor {
    /// The cursor after `polled`, a poll's answer, with the validators the
    /// server returned. It remembers the newest `remembered` identities
    /// only, enough to cover any one poll.
    #[must_use]
    pub fn advanced(&self, polled: &Polled, remembered: usize) -> Self {
        let found = &polled.events;
        let mut seen: Vec<Identity> = found.iter().map(|event| event.identity().clone()).collect();
        seen.extend(
            polled
                .passed
                .iter()
                .chain(&polled.unread)
                .filter(|identity| !seen_in(found, identity))
                .cloned(),
        );
        let newest = seen.clone();
        seen.extend(
            self.seen
                .iter()
                .filter(|identity| !newest.contains(identity))
                .cloned(),
        );
        seen.retain(|identity| !polled.withdrawn.contains(identity));
        seen.truncate(remembered);
        let released = |identity: &Identity| {
            found.iter().any(|event| {
                event.identity() == identity && event.signal() == Some(Signal::Release)
            })
        };
        let mut tagged: Vec<Identity> = Vec::new();
        let fresh_tags = found
            .iter()
            .filter(|event| event.signal() == Some(Signal::Tag))
            .map(Event::identity)
            .chain(&polled.passed);
        for identity in fresh_tags.chain(&self.tagged) {
            if !released(identity) && seen.contains(identity) && !tagged.contains(identity) {
                tagged.push(identity.clone());
            }
        }
        let notes = self.watched(polled, &seen);
        let taken = polled.taken();
        let mut projects = self.projects.clone();
        projects.extend(
            found
                .iter()
                .filter(|event| taken.contains(event.identity()))
                .map(|event| (event.identity().clone(), event.project().to_owned())),
        );
        projects.retain(|identity, _| seen.contains(identity));
        Self {
            seen,
            tagged,
            etag: polled.etag.clone(),
            last_modified: polled.last_modified.clone(),
            project: found
                .first()
                .map(|event| event.project().to_owned())
                .or_else(|| self.project.clone()),
            signals: polled.signals.clone(),
            notes,
            packages: polled.packages.clone(),
            listings: polled.listings.clone(),
            newest: polled.newest.or(self.newest),
            sequence: polled.sequence.or(self.sequence),
            log: polled.log.or(self.log),
            ahead: polled.ahead.clone(),
            proved: self.proofs(polled, remembered),
            projects,
            advised: self.advised.clone(),
            pending: self.pending.clone(),
        }
    }

    /// The cursor after `polled`, what a peer pushed rather than what a
    /// poll read: everything a poll keeps of its own, such as its entity
    /// tag and place in the feed, stays as it was (ADR-0054).
    #[must_use]
    pub fn received(&self, polled: &Polled, remembered: usize) -> Self {
        let advanced = self.advanced(polled, remembered);
        Self {
            etag: self.etag.clone(),
            last_modified: self.last_modified.clone(),
            project: self.project.clone().or(advanced.project),
            signals: self.signals.clone(),
            packages: self.packages.clone(),
            listings: self.listings.clone(),
            newest: self.newest,
            sequence: self.sequence,
            log: self.log,
            ahead: self.ahead.clone(),
            ..advanced
        }
    }

    /// When the proof of the newest envelope of each identity it took was
    /// made, `polled`'s taken after its own: every tombstone of an identity
    /// it took; the tombstones of others taken latest, as many as
    /// `polled.unheld` keeps; and of the rest those taken latest, as many
    /// of each instance as `polled.each` keeps, else `remembered` of all.
    fn proofs(&self, polled: &Polled, remembered: usize) -> Vec<Proof> {
        let kept: BTreeMap<&Identity, &Proof> = self
            .proved
            .iter()
            .map(|proof| (&proof.identity, proof))
            .collect();
        let taken = polled.taken();
        let mut proved: Vec<Proof> = polled
            .proved
            .iter()
            .map(|proof| match kept.get(&proof.identity) {
                Some(kept) if proof.created < kept.created => (*kept).clone(),
                _ => proof.clone(),
            })
            .collect();
        proved.extend(
            self.proved
                .iter()
                .filter(|proof| !taken.contains(&proof.identity))
                .cloned(),
        );
        let mut live: BTreeMap<&str, usize> = BTreeMap::new();
        let mut unheld = 0_usize;
        let mut each: BTreeMap<&str, usize> = BTreeMap::new();
        let keeps: Vec<bool> = proved
            .iter()
            .map(|proof| match &proof.tombstone {
                Some(Tombstone::Held) => true,
                Some(Tombstone::Unheld) => {
                    let made = each.entry(&proof.maker).or_default();
                    let keep = unheld < polled.unheld.most && *made < polled.unheld.each;
                    if keep {
                        unheld += 1;
                        *made += 1;
                    }
                    keep
                }
                None => {
                    let maker = polled.each.map_or("", |_| proof.maker.as_str());
                    let made = live.entry(maker).or_default();
                    *made += 1;
                    *made <= polled.each.unwrap_or(remembered)
                }
            })
            .collect();
        proved
            .into_iter()
            .zip(keeps)
            .filter_map(|(proof, keep)| keep.then_some(proof))
            .collect()
    }

    /// How many tombstones of identities it took it keeps of each
    /// instance.
    #[must_use]
    pub fn held(&self) -> BTreeMap<&str, usize> {
        let mut held: BTreeMap<&str, usize> = BTreeMap::new();
        for proof in self.proved.iter().filter(|proof| proof.held()) {
            *held.entry(proof.maker.as_str()).or_default() += 1;
        }
        held
    }

    /// The project of the event `identity` it took from another herald's
    /// feed, where it still sees it.
    #[must_use]
    pub fn project_of(&self, identity: &Identity) -> Option<&str> {
        self.projects.get(identity).map(String::as_str)
    }

    /// The proof of the newest envelope of `identity` it took.
    #[must_use]
    pub fn proof(&self, identity: &Identity) -> Option<&Proof> {
        self.proved.iter().find(|proof| proof.identity == *identity)
    }

    /// Whether it took an event of `identity`: it keeps the proof of an
    /// envelope of it whose event it did not pass over, or of a tombstone
    /// withdrawing such an event.
    #[must_use]
    pub fn took(&self, identity: &Identity) -> bool {
        self.proof(identity)
            .is_some_and(|proof| match proof.tombstone {
                Some(Tombstone::Held) => true,
                Some(Tombstone::Unheld) => false,
                None => !proof.passed,
            })
    }

    /// Whether an envelope of `identity` whose proof was made at `created`
    /// is newer than the newest it took of it.
    #[must_use]
    pub fn newer(&self, identity: &Identity, created: Timestamp) -> bool {
        self.proof(identity)
            .is_none_or(|kept| created > kept.created)
    }

    /// The digests of the newest Releases `polled` found, as many as it
    /// watches, where it found any; else those it watched before. Where
    /// its source relists, those it watched before stay beside them for
    /// every identity still `seen`.
    fn watched(&self, polled: &Polled, seen: &[Identity]) -> BTreeMap<Identity, String> {
        let mut releases = polled
            .events
            .iter()
            .filter(|event| event.signal() == Some(Signal::Release))
            .peekable();
        if polled.relisted {
            let mut notes = self.notes.clone();
            notes.extend(
                releases
                    .take(polled.watched)
                    .map(|event| (event.identity().clone(), noted(event))),
            );
            notes.retain(|identity, _| seen.contains(identity));
            return notes;
        }
        if releases.peek().is_none() {
            return self.notes.clone();
        }
        releases
            .take(polled.watched)
            .map(|event| (event.identity().clone(), noted(event)))
            .collect()
    }

    /// Whether `event` is a Release it watches whose name or notes changed
    /// since.
    #[must_use]
    pub fn edited(&self, event: &Event) -> bool {
        event.signal() == Some(Signal::Release)
            && self
                .notes
                .get(event.identity())
                .is_some_and(|digest| *digest != noted(event))
    }

    /// Whether `event` is one it has not seen: an identity it has not seen,
    /// or the Release of a version it has seen only as a tag.
    #[must_use]
    pub fn unseen(&self, event: &Event) -> bool {
        let identity = event.identity();
        !self.seen.contains(identity)
            || (event.signal() == Some(Signal::Release) && self.tagged.contains(identity))
    }

    /// The validators the server last returned for `signal`'s request, where
    /// it returned any.
    #[must_use]
    pub fn validators(&self, signal: Signal) -> Option<&Validators> {
        self.signals.get(&signal)
    }
}

/// The digest of `event`'s name and notes.
fn noted(event: &Event) -> String {
    digest(&format!(
        "{}\n{}\n{}",
        event.title().len(),
        event.title(),
        event.body()
    ))
}

fn seen_in(found: &[Event], identity: &Identity) -> bool {
    found.iter().any(|event| event.identity() == identity)
}

/// The events of a poll to announce: those `cursor` has not seen, each
/// Release of a version it has seen only as a tag, and each Release it
/// watches whose name or notes changed, marked edited. With no cursor, the
/// source is new, and only its newest `backfill` are announced.
#[must_use]
pub fn fresh(cursor: Option<&Cursor>, found: Vec<Event>, backfill: usize) -> Vec<Event> {
    match cursor {
        Some(cursor) => found
            .into_iter()
            .filter_map(|event| {
                if cursor.unseen(&event) {
                    Some(event)
                } else if cursor.edited(&event) {
                    Some(event.marked_edited())
                } else {
                    None
                }
            })
            .collect(),
        None => found.into_iter().take(backfill).collect(),
    }
}

#[cfg(test)]
mod tests;

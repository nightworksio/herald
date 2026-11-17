//! An event published to a route's feed of herald's own (ADR-0046): as an
//! envelope proved by the instance's newest key, listed first in the feed
//! the store keeps, once for each revision the route shows.

use herald_core::{
    Entry, Event, Facts, Feed, FeedItem, Key, Revise, Revision, Step, Visibility, revise,
};
use herald_keys::ed25519_of;
use herald_kinds::Fed;
use herald_peers::discovery::Instance;
use herald_peers::feed::{Envelope, identity, shows, withdrawn};
use herald_peers::jcs::canonical;
use herald_peers::keys::method;
use herald_peers::proof::unproved;
use herald_ports::{Algorithm, Ledger, Published, Signer, Store, Versioned, Written};

use super::{Outcome, Run, Stop, stopped, stored};

/// Why an event that is not public enters no feed.
const PRIVATE: &str =
    "the event is not public, and a herald feed is served to anyone, so it enters none";

impl<S: Store, C: herald_ports::Clock, G: Signer, H> Run<'_, S, C, G, H> {
    /// What the route did with `event`, published to its feed `fed`: where
    /// the routing saw it as anything but public, its envelope withdrawn
    /// from the feed where the feed lists one; left out where the route
    /// shows it already; and otherwise entered first in the feed and landed
    /// as the route's post.
    pub(crate) async fn fed(&self, fed: Fed<'_>, event: &Event) -> Outcome {
        let key = Key::derive(&self.route.config.route.id, event.identity());
        let entry = match self.ports.store.ledger().entry(&key).await {
            Ok(entry) => entry,
            Err(error) => return stopped(stored(error)),
        };
        if self.seen != Visibility::Public {
            return self.withheld(fed, event, entry).await;
        }
        let instance = self.route.instance();
        let envelope =
            match instance.envelope(event, self.kinds.get(event.kind()), fed.feeding.envelope) {
                Ok(envelope) => envelope,
                Err(why) => return stopped(Stop::Refused(why.to_string())),
            };
        let revision = Revision::of(
            event.signal(),
            [canonical(&envelope).unwrap_or_default().as_str()],
        );
        let delivery = &self.route.config.route.delivery;
        let posted = entry
            .as_ref()
            .map(|entry| &entry.value)
            .filter(|entry| entry.message(&Step::Post).is_some());
        match posted {
            None if event.edited() => return Outcome::Unedited,
            Some(shown) if revise(shown, &revision, delivery, true) == Revise::Keep => {
                return Outcome::AlreadyAnnounced;
            }
            _ => {}
        }
        let shown =
            |item: Option<&FeedItem>| item.is_some_and(|item| shows(&item.envelope, &envelope));
        let entered = match self.entered(fed, &instance, &envelope, shown).await {
            Ok(entered) => entered.unwrap_or_default(),
            Err(reason) => return stopped(reason),
        };
        let landed = self
            .fed_entry(key, entry.as_ref())
            .revising(revision)
            .landed(
                Step::Post,
                Facts {
                    message: Some(entered.to_string()),
                    ..Facts::default()
                },
            );
        self.recorded(
            &landed,
            entry,
            Outcome::Sent {
                steps: vec![Step::Post],
            },
        )
        .await
    }

    /// What the route did with `event`, which the routing saw as anything
    /// but public: its envelope withdrawn by a tombstone where the feed
    /// lists one, and the route's post forgotten, so the event enters again
    /// once it is public; left out in any case.
    async fn withheld(
        &self,
        fed: Fed<'_>,
        event: &Event,
        entry: Option<Versioned<Entry>>,
    ) -> Outcome {
        let identity = event.identity().as_str();
        let destination = &self.route.config.route.destination;
        let gone = |item: Option<&FeedItem>| item.is_none_or(|item| withdrawn(&item.envelope));
        let listed = match self.ports.store.published().feed(destination).await {
            Ok(listed) => listed,
            Err(error) => return stopped(stored(error)),
        };
        let skipped = Outcome::Skipped(PRIVATE.to_owned());
        if gone(
            listed
                .as_ref()
                .and_then(|listed| listed.value.held(identity)),
        ) {
            return skipped;
        }
        let instance = self.route.instance();
        let tombstone = instance.tombstone(identity, self.ports.clock.now());
        if let Err(reason) = self.entered(fed, &instance, &tombstone, gone).await {
            return stopped(reason);
        }
        let key = Key::derive(&self.route.config.route.id, event.identity());
        let forgotten = self.fed_entry(key, None);
        self.recorded(&forgotten, entry, skipped).await
    }

    /// The route's entry for `key`, kept where `entry` keeps it, claimed and
    /// released by this run now: a new one with nothing landed where
    /// `entry` is none.
    fn fed_entry(&self, key: Key, entry: Option<&Versioned<Entry>>) -> Entry {
        let now = self.ports.clock.now();
        entry
            .map_or_else(|| Entry::new(key), |entry| entry.value.clone())
            .claimed_by(self.holder, now, now)
            .released()
    }

    /// `outcome`, once `written` is kept over `entry`; what another run's
    /// write first, or the store, made of it otherwise.
    async fn recorded(
        &self,
        written: &Entry,
        entry: Option<Versioned<Entry>>,
        outcome: Outcome,
    ) -> Outcome {
        let expected = entry.map(|entry| entry.version);
        match self.ports.store.ledger().put_entry(written, expected).await {
            Ok(Written::Written(_)) => outcome,
            Ok(Written::Conflict) => Outcome::AlreadyAnnounced,
            Err(error) => stopped(stored(error)),
        }
    }

    /// The sequence the route's feed lists `envelope` under, proved and
    /// entered first, where what the feed lists of its event is not yet
    /// `settled`; or what it lists already, where it is. Each write is made
    /// with compare-and-set within the store's attempts.
    async fn entered(
        &self,
        fed: Fed<'_>,
        instance: &Instance<'_>,
        envelope: &Envelope,
        settled: impl Fn(Option<&FeedItem>) -> bool,
    ) -> Result<Option<u64>, Stop> {
        let destination = &self.route.config.route.destination;
        let event = identity(envelope);
        let proved = self.proved(fed, instance, envelope).await?;
        let published = self.ports.store.published();
        for _ in 0..self.route.attempts {
            let held = published.feed(destination).await.map_err(stored)?;
            let listing = held
                .as_ref()
                .map_or_else(Feed::default, |held| held.value.clone());
            let item = listing.held(event);
            if settled(item) {
                return Ok(item.map(|item| item.sequence));
            }
            let next = listing.entered(proved.clone(), fed.feeding.keep);
            let expected = held.map(|held| held.version);
            match published
                .put_feed(destination, &next, expected)
                .await
                .map_err(stored)?
            {
                Written::Written(_) => return Ok(Some(next.newest)),
                Written::Conflict => {}
            }
        }
        Err(Stop::Store(format!(
            "the feed was written by another run each of {} times herald wrote it",
            self.route.attempts
        )))
    }

    /// `envelope` proved by the instance's newest key, `fed.key`, as of now.
    async fn proved(
        &self,
        fed: Fed<'_>,
        instance: &Instance<'_>,
        envelope: &Envelope,
    ) -> Result<Envelope, Stop> {
        let signer = self.ports.signer;
        let spki = signer
            .public(Algorithm::Ed25519, fed.key)
            .await
            .map_err(unprovable)?;
        let public = ed25519_of(&spki).ok_or_else(|| {
            Stop::Proof(format!(
                "the federation key {} is not an Ed25519 key",
                fed.key.name()
            ))
        })?;
        let ready = unproved(
            envelope,
            &method(instance.domain, &public),
            self.ports.clock.now(),
        )
        .map_err(unprovable)?;
        let signature = signer
            .sign(Algorithm::Ed25519, fed.key, ready.message())
            .await
            .map_err(unprovable)?;
        Ok(ready.proved(&signature))
    }
}

/// Why an envelope could not be proved: `error`, as it says.
fn unprovable(error: impl std::fmt::Display) -> Stop {
    Stop::Proof(error.to_string())
}

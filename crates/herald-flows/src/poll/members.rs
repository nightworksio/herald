//! An owner source's repositories listed each round, and each member it
//! takes polled on its own (ADR-0029).

use std::collections::{BTreeMap, BTreeSet};

use futures_util::stream::{self, StreamExt};
use herald_config::{Config, Guard};
use herald_core::{
    Cursor, Listed, Listing, Membership, Origin, Owner, Polled, RouteId, SourceName, Visibility,
};
use herald_kinds::{Changed, Expand, Expanded, Rounded, Unlisted};
use herald_ports::{Clock, Cursors, Http, Signer, Store, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use super::claims::Claims;
use super::{Remembered, Sweep, Swept, announced_found, halted, remembered, stored};
use crate::announce::{Ports, Stop, Within, at_once};
use crate::prepare::{Connected, PreparedSource};

/// One member as this round holds it: its membership, the version it was
/// read at, and whether this round changed it.
struct Held {
    membership: Membership,
    version: Option<Version>,
    changed: bool,
}

/// What reconciling a listing with the kept members did.
struct Reconciled {
    held: BTreeMap<String, Held>,
    joined: Vec<String>,
    left: Vec<String>,
    forgotten: BTreeSet<String>,
}

impl Reconciled {
    /// Each member taken whose project `claims` finds another announcing as
    /// marked withheld, and each no longer so cleared, as a member of the
    /// owner source `owner`.
    fn withhold(&mut self, (owner, expand): (&SourceName, &dyn Expand), claims: &Claims) {
        for held in self.held.values_mut() {
            let collides = held
                .membership
                .left
                .is_none()
                .then(|| claims.collision((owner, expand), &held.membership.repository))
                .flatten();
            if collides != held.membership.collides {
                held.membership.collides = collides;
                held.changed = true;
            }
        }
    }
}

/// The kept members, brought to what `listed` takes at `now`: a member
/// listed again is updated and taken back, one never seen joins, one no
/// longer listed leaves, and one that left before `forget` is forgotten.
fn reconcile(
    kept: Vec<Versioned<Membership>>,
    (listed, expand): (Option<Vec<Listed>>, &dyn Expand),
    now: Timestamp,
    forget: Timestamp,
) -> Reconciled {
    let mut held: BTreeMap<String, Held> = kept
        .into_iter()
        .map(|kept| {
            (
                kept.value.repository.id.clone(),
                Held {
                    membership: kept.value,
                    version: Some(kept.version),
                    changed: false,
                },
            )
        })
        .collect();
    let (mut joined, mut left) = (Vec::new(), Vec::new());
    if let Some(listed) = listed {
        let ids: BTreeSet<String> = listed.iter().map(|listed| listed.id.clone()).collect();
        for repository in listed {
            if let Some(member) = held.get_mut(&repository.id) {
                if member.membership.left.is_some() {
                    joined.push(repository.path.clone());
                }
                member.changed |=
                    member.membership.repository != repository || member.membership.left.is_some();
                member.membership.repository = repository;
                member.membership.left = None;
            } else {
                joined.push(repository.path.clone());
                held.insert(
                    repository.id.clone(),
                    Held {
                        membership: Membership {
                            repository,
                            discovered: now,
                            left: None,
                            cursor: None,
                            collides: None,
                            hooked: false,
                        },
                        version: None,
                        changed: true,
                    },
                );
            }
        }
        for (id, member) in &mut held {
            if member.membership.left.is_none() && !ids.contains(id) {
                left.push(member.membership.repository.path.clone());
                member.membership.left = Some(now);
                member.changed = true;
            }
        }
    }
    for member in held.values_mut() {
        if member.membership.left.is_none() && !expand.takes(&member.membership.repository) {
            left.push(member.membership.repository.path.clone());
            member.membership.left = Some(now);
            member.changed = true;
        }
    }
    let forgotten: BTreeSet<String> = held
        .iter()
        .filter(|(_, member)| member.membership.left.is_some_and(|at| at < forget))
        .map(|(id, _)| id.clone())
        .collect();
    held.retain(|id, _| !forgotten.contains(id));
    Reconciled {
        held,
        joined,
        left,
        forgotten,
    }
}

/// The routes a member's events may reach where they are bounded: where
/// `guard` refuses, a repository not everyone may see reaches only the
/// `private` routes.
fn reach<'a>(
    guard: Guard,
    visibility: Visibility,
    private: impl Iterator<Item = &'a RouteId>,
) -> Option<Vec<RouteId>> {
    (guard == Guard::Refuse && visibility != Visibility::Public).then(|| private.cloned().collect())
}

/// The routes `config` lets a member of `visibility` reach where they are
/// bounded (ADR-0029).
pub(crate) fn bounded(config: &Config, visibility: Visibility) -> Option<Vec<RouteId>> {
    reach(
        config.doctor.private_to_public,
        visibility,
        config
            .routes
            .iter()
            .filter(|route| route.private)
            .map(|route| &route.route.id),
    )
}

/// `polled`, each of its events from `origin`.
fn originated(polled: Option<Polled>, origin: &Origin) -> Option<Polled> {
    polled.map(|polled| Polled {
        events: polled
            .events
            .into_iter()
            .map(|event| event.from_origin(origin.clone()))
            .collect(),
        ..polled
    })
}

/// The member `held`, whose poll answered `polled`, announced, and its
/// cursor written where it moved.
async fn member<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (source, expand): (&PreparedSource, &dyn Expand),
    (mut held, first): (Held, Timestamp),
    polled: Result<Option<Polled>, Stop>,
    run: &str,
) -> Swept {
    let ports = flow.2;
    let swept = |outcome| Swept {
        source: source.name.clone(),
        member: Some(held.membership.repository.path.clone()),
        outcome,
    };
    let found = match polled {
        Err(stop) => Err(stop),
        Ok(polled) => {
            let holder = format!(
                "{run}/{}/{}",
                source.name.as_str(),
                held.membership.repository.id
            );
            let since = (held.membership.discovered > first).then_some(held.membership.discovered);
            let repository = &held.membership.repository;
            let reach = expand.reach(repository);
            let polled = originated(polled, &expand.origin(repository));
            let holding = expand.tag_window(repository).map(|window| super::Holding {
                window,
                member: Some(&repository.id),
            });
            announced_found(
                flow,
                (
                    &source.name,
                    Within {
                        routes: None,
                        reach: Some(&reach),
                    },
                    holding,
                ),
                (polled, expand.remembered()),
                (held.membership.cursor.as_ref(), since, source.backfill),
                &holder,
            )
            .await
        }
    };
    let outcome = match found {
        Err(stop) => Sweep::Stopped(stop),
        Ok(None) => Sweep::Unchanged,
        Ok(Some(found)) => {
            let first = held.membership.cursor.is_none();
            let cursor = match found.advanced {
                Some(cursor) => {
                    held.membership.cursor = Some(cursor);
                    remembered(
                        ports
                            .store
                            .cursors()
                            .put_member(&source.name, &held.membership, held.version)
                            .await,
                        "the member's cursor",
                    )
                }
                None => Remembered::Kept,
            };
            Sweep::Polled {
                first,
                found: found.count,
                announced: found.announced,
                cursor,
            }
        }
    };
    swept(outcome)
}

/// The cursor a member that joined after the first listing is polled with,
/// so its poll reads in full everything it finds, which announces what
/// it published since it joined.
static JOINED: Cursor = Cursor {
    seen: Vec::new(),
    etag: None,
    last_modified: None,
    project: None,
    signals: std::collections::BTreeMap::new(),
    tagged: Vec::new(),
    notes: std::collections::BTreeMap::new(),
    packages: std::collections::BTreeMap::new(),
    listings: std::collections::BTreeMap::new(),
    newest: None,
    sequence: None,
    ahead: Vec::new(),
    proved: Vec::new(),
};

/// The members `chunk` polled together through `http`, as one request
/// where the source batches them, and each announced in turn.
async fn chunk<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    source: &PreparedSource,
    (expand, http, rounded): (&dyn Expand, &H, &Rounded),
    (chunk, first): (Vec<Held>, Timestamp),
    run: &str,
) -> Vec<Swept> {
    let ports = flow.2;
    let polled: Vec<Result<Option<Polled>, Stop>> =
        if let Err(stop) = ports.budget.allows(ports.clock.now(), 1) {
            vec![Err(stop); chunk.len()]
        } else {
            let asked: Vec<(&Listed, Option<&Cursor>)> = chunk
                .iter()
                .map(|held| {
                    let joined = held.membership.discovered > first;
                    let cursor = held
                        .membership
                        .cursor
                        .as_ref()
                        .or_else(|| joined.then_some(&JOINED));
                    (&held.membership.repository, cursor)
                })
                .collect();
            expand
                .polled(http, &asked, rounded, ports.clock.now())
                .await
                .into_iter()
                .map(|answered| answered.map_err(halted))
                .collect()
        };
    let mut swept = Vec::new();
    for (held, polled) in chunk.into_iter().zip(polled) {
        swept.push(member(flow, (source, expand), (held, first), polled, run).await);
    }
    swept
}

/// `takers` split into those polled this round and those left quiet: a
/// member is polled where the owner's events are not followed, it has
/// never been polled, or the events `named` or the round's read `rounded`
/// names it.
fn split(
    takers: Vec<Held>,
    named: Option<&BTreeSet<String>>,
    rounded: &Rounded,
) -> (Vec<Held>, Vec<Held>) {
    takers.into_iter().partition(|held| {
        let id = &held.membership.repository.id;
        named.is_none_or(|ids| {
            held.membership.cursor.is_none()
                || ids.contains(id)
                || rounded.named().is_some_and(|named| named.contains(id))
        })
    })
}

/// `takers` in runs of `size`, in order.
fn chunked(takers: Vec<Held>, size: usize) -> Vec<Vec<Held>> {
    let mut chunks: Vec<Vec<Held>> = Vec::new();
    for held in takers {
        match chunks.last_mut() {
            Some(last) if last.len() < size => last.push(held),
            _ => chunks.push(vec![held]),
        }
    }
    chunks
}

/// What the owner `expand` lists through `http` at `now`, conditional on
/// `known`: the members it takes and the listing to keep, or neither where
/// it is unchanged, but for `known` no longer stale where it was.
async fn listed(
    expand: &dyn Expand,
    http: &dyn herald_ports::DynHttp,
    known: Option<&Listing>,
    now: Timestamp,
) -> Result<(Option<Vec<Listed>>, Option<Listing>), Unlisted> {
    Ok(match expand.listed(http, known).await? {
        Expanded::Unchanged => (
            None,
            known.filter(|known| known.stale).map(|known| Listing {
                stale: false,
                ..known.clone()
            }),
        ),
        Expanded::Listed {
            members,
            owner,
            pages,
            file,
            ..
        } => (
            Some(members),
            Some(Listing {
                owner,
                pages,
                first: known.map_or(now, |known| known.first),
                listed: now,
                fingerprint: expand.fingerprint(),
                file,
                events: known.and_then(|known| known.events.clone()),
                stale: false,
            }),
        ),
    })
}

/// An owner source's round as far as its reconciliation: its listing as
/// read and as listed now, when it was first listed, its members brought to
/// the listing, what the round reported so far, and whether a cap refused
/// it.
pub(super) struct Owning {
    record: Option<Versioned<Listing>>,
    listing: Option<Listing>,
    first: Timestamp,
    reconciled: Reconciled,
    swept: Vec<Swept>,
    refused: bool,
}

impl Owning {
    /// Each member it takes.
    pub(super) fn taken(&self) -> impl Iterator<Item = &Listed> {
        self.reconciled
            .held
            .values()
            .filter(|held| held.membership.left.is_none())
            .map(|held| &held.membership.repository)
    }
}

/// The owner `expand` lists, through `http`, reconciled with the members
/// `source` keeps. A listing over a cap refuses the source for the round,
/// keeping its members as they were; one that fails keeps the last
/// membership, adding and dropping none. A source discovering by push
/// lists only where its listing is missing, stale or of another selection.
/// Where the source's records cannot be read, what the round reports.
pub(super) async fn reconciled<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (source, expand, http): (&PreparedSource, &dyn Expand, &H),
) -> Result<Owning, Vec<Swept>> {
    let ports = flow.2;
    let told = |outcome| Swept {
        source: source.name.clone(),
        member: None,
        outcome,
    };
    let now = ports.clock.now();
    ports
        .budget
        .allows(now, 1)
        .map_err(|stop| vec![told(Sweep::Stopped(stop))])?;
    let cursors = ports.store.cursors();
    let read = async {
        Ok::<_, StoreError>((
            cursors.listing(&source.name).await?,
            cursors.members(&source.name).await?,
        ))
    };
    let (record, kept) = read
        .await
        .map_err(|error| vec![told(Sweep::Stopped(stored(error)))])?;
    let known = record.as_ref().map(|record| &record.value);
    let mut swept = Vec::new();
    let mut refused = false;
    let fresh = expand.pushed()
        && known.is_some_and(|known| !known.stale && known.fingerprint == expand.fingerprint());
    let listing = if fresh {
        Ok((None, None))
    } else {
        listed(expand, http, known, now).await
    };
    let (listed, listing) = match listing {
        Ok(listed) => listed,
        Err(Unlisted::Over(reason)) => {
            refused = true;
            swept.push(told(Sweep::Stopped(Stop::Source(reason))));
            (None, None)
        }
        Err(Unlisted::Failed(unpolled)) => {
            swept.push(told(Sweep::Stopped(halted(unpolled))));
            (None, None)
        }
    };
    let first = listing
        .as_ref()
        .or(known)
        .map_or(now, |listing| listing.first);
    let reconciled = reconcile(kept, (listed, expand), now, now - expand.keep_members());
    Ok(Owning {
        record,
        listing,
        first,
        reconciled,
        swept,
        refused,
    })
}

/// The members the owner's events name since they were last read, where
/// the source follows them, and the listing to keep where how far they
/// were read moved; a read that fails is reported through `told` into
/// `swept`, and every member is polled.
async fn followed<S: Store, C: Clock, G: Signer, H: Http>(
    (ports, expand, http): (&Ports<'_, S, C, G>, &dyn Expand, &H),
    (known, listing): (Option<&Listing>, Option<&Listing>),
    swept: &mut Vec<Swept>,
    told: impl Fn(Sweep) -> Swept,
) -> (Option<BTreeSet<String>>, Option<Listing>) {
    let owner = listing
        .or(known)
        .map_or(Owner::Organization, |listing| listing.owner);
    let last = known.and_then(|known| known.events.as_ref());
    let changed = match ports.budget.allows(ports.clock.now(), 1) {
        Ok(()) => expand.changed(http, owner, last).await,
        Err(_) => None,
    };
    let (named, events) = match changed {
        None => (None, None),
        Some(Ok(Changed::Named { ids, read })) => (Some(ids), Some(read)),
        Some(Ok(Changed::Gap(read))) => (None, Some(read)),
        Some(Err(unpolled)) => {
            swept.push(told(Sweep::Stopped(halted(unpolled))));
            (None, None)
        }
    };
    let kept = events.filter(|read| Some(read) != last).and_then(|read| {
        listing.or(known).map(|listing| Listing {
            events: Some(read),
            ..listing.clone()
        })
    });
    (named, kept)
}

/// What the owner `expand`, of the kind `owner`, reads through `http` once
/// a round before any member is polled; a read that fails is reported
/// through `told` into `swept`, and where it fails or the budget leaves no
/// room for it, each member's poll leaves out what it would have read.
async fn rounded<S: Store, C: Clock, G: Signer, H: Http>(
    (ports, expand, http): (&Ports<'_, S, C, G>, &dyn Expand, &H),
    owner: Owner,
    swept: &mut Vec<Swept>,
    told: impl Fn(Sweep) -> Swept,
) -> Rounded {
    if ports.budget.allows(ports.clock.now(), 1).is_err() {
        return Rounded::Unread;
    }
    expand
        .rounded(http, owner)
        .await
        .unwrap_or_else(|unpolled| {
            swept.push(told(Sweep::Stopped(halted(unpolled))));
            Rounded::Unread
        })
}

/// Whether every member of `forgotten` was forgotten with its cursor by the
/// owner source `source`; each that could not be is reported into `swept`.
async fn forgot<S: Store>(
    store: &S,
    source: &SourceName,
    forgotten: &BTreeSet<String>,
    swept: &mut Vec<Swept>,
) -> bool {
    let mut whole = true;
    for id in forgotten {
        if let Err(error) = store.cursors().forget_member(source, id).await {
            whole = false;
            swept.push(told(source, None, Sweep::Stopped(stored(error))));
        }
    }
    whole
}

/// Each of `takers` given a webhook where the round registers one on a
/// member that has none, its membership written once it has one; a member
/// that could not be given one is reported into `swept`, and asked again
/// next round.
async fn hooked<S: Store, C: Clock, G: Signer, H: Http>(
    (ports, expand, http): (&Ports<'_, S, C, G>, &dyn Expand, &H),
    source: &PreparedSource,
    mut takers: Vec<Held>,
    swept: &mut Vec<Swept>,
) -> Vec<Held> {
    let Some((url, Some(secret))) = &source.hook else {
        return takers;
    };
    let cursors = ports.store.cursors();
    for held in takers.iter_mut().filter(|held| !held.membership.hooked) {
        if ports.budget.allows(ports.clock.now(), 2).is_err() {
            break;
        }
        match expand
            .register(http, &held.membership.repository, (url, secret))
            .await
        {
            Ok(_) => {
                held.membership.hooked = true;
                if let Ok(Written::Written(version)) = cursors
                    .put_member(&source.name, &held.membership, held.version)
                    .await
                {
                    held.version = Some(version);
                }
            }
            Err(unpolled) => swept.push(told(
                &source.name,
                Some(&held.membership.repository.path),
                Sweep::Stopped(halted(unpolled)),
            )),
        }
    }
    takers
}

/// What the owner source `source`, or its member at `member`, did.
fn told(source: &SourceName, member: Option<&str>, outcome: Sweep) -> Swept {
    Swept {
        source: source.clone(),
        member: member.map(ToOwned::to_owned),
        outcome,
    }
}

/// The owner source's round `owning` finished: each member whose project
/// `claims` finds another announcing as withheld, every member it changed
/// written, then its listing, and each member it takes polled. The listing
/// is written only once every member it changed is, so a listing the next
/// round finds unchanged never hides a member.
pub(super) async fn finished<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (source, expand, http): (&PreparedSource, &dyn Expand, &H),
    (owning, claims): (Owning, &Claims),
    run: &str,
) -> Vec<Swept> {
    let (config, _, ports) = flow;
    let Owning {
        record,
        listing,
        first,
        mut reconciled,
        mut swept,
        refused,
    } = owning;
    if refused {
        return swept;
    }
    let told = |outcome| told(&source.name, None, outcome);
    reconciled.withhold((&source.name, expand), claims);
    let cursors = ports.store.cursors();
    let mut whole = forgot(ports.store, &source.name, &reconciled.forgotten, &mut swept).await;
    let mut takers = Vec::new();
    let mut withheld = Vec::new();
    for mut held in reconciled.held.into_values() {
        if held.changed {
            match cursors
                .put_member(&source.name, &held.membership, held.version)
                .await
            {
                Ok(Written::Written(version)) => held.version = Some(version),
                written => {
                    whole = false;
                    if let Remembered::Failed(stop) = remembered(written, "the membership") {
                        swept.push(told(Sweep::Stopped(stop)));
                    }
                    continue;
                }
            }
        }
        match (&held.membership.left, &held.membership.collides) {
            (Some(_), _) => {}
            (None, Some(reason)) => withheld.push(self::told(
                &source.name,
                Some(&held.membership.repository.path),
                Sweep::Stopped(Stop::Source(reason.clone())),
            )),
            (None, None) => takers.push(held),
        }
    }
    let (named, followed) = followed(
        (flow.2, expand, http),
        (
            record.as_ref().map(|record| &record.value),
            listing.as_ref(),
        ),
        &mut swept,
        told,
    )
    .await;
    let owner = listing
        .as_ref()
        .or(record.as_ref().map(|record| &record.value))
        .map_or(Owner::Organization, |listing| listing.owner);
    if let Some(listing) = followed.or(listing).filter(|_| whole) {
        let expected = record.as_ref().map(|record| record.version);
        if let Remembered::Failed(stop) = remembered(
            cursors.put_listing(&source.name, &listing, expected).await,
            "the owner's listing",
        ) {
            swept.push(told(Sweep::Stopped(stop)));
        }
    }
    let takers = hooked((flow.2, expand, http), source, takers, &mut swept).await;
    let rounded = rounded((flow.2, expand, http), owner, &mut swept, told).await;
    let (takers, quiet) = split(takers, named.as_ref(), &rounded);
    swept.push(told(Sweep::Listed {
        members: takers.len() + quiet.len() + withheld.len(),
        joined: reconciled.joined,
        left: reconciled.left,
    }));
    swept.extend(withheld);
    swept.extend(quiet.iter().map(|held| {
        self::told(
            &source.name,
            Some(&held.membership.repository.path),
            Sweep::Unchanged,
        )
    }));
    let polled: Vec<Vec<Swept>> = stream::iter(
        chunked(takers, expand.batch().get())
            .into_iter()
            .map(|held| chunk(flow, source, (expand, http, &rounded), (held, first), run)),
    )
    .buffered(at_once(ports.store, config.run.sources).get())
    .collect()
    .await;
    swept.extend(polled.into_iter().flatten());
    swept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_not_everyone_may_see_reaches_only_private_routes_where_doctor_refuses() {
        let private = [RouteId::new("team")];
        assert_eq!(
            reach(Guard::Refuse, Visibility::Private, private.iter()),
            Some(private.to_vec())
        );
        assert_eq!(
            reach(Guard::Refuse, Visibility::Internal, std::iter::empty()),
            Some(Vec::new())
        );
        assert_eq!(
            reach(Guard::Refuse, Visibility::Public, private.iter()),
            None
        );
        assert_eq!(
            reach(Guard::Warn, Visibility::Private, private.iter()),
            None
        );
        assert_eq!(
            reach(Guard::Allow, Visibility::Private, private.iter()),
            None
        );
    }
}

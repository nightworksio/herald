//! An owner source's repositories listed each round, and each member it
//! takes polled on its own (ADR-0029).

use std::collections::{BTreeMap, BTreeSet};

use futures_util::stream::{self, StreamExt};
use herald_config::Config;
use herald_core::{Listed, Listing, Membership};
use herald_kinds::{Expand, Expanded, Unlisted, Unpolled};
use herald_ports::{Clock, Cursors, Http, Signer, Store, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use super::{Remembered, Sweep, Swept, found, remembered, stored};
use crate::announce::{Ports, Stop, at_once};
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

/// The kept members, brought to what `listed` takes at `now`: a member
/// listed again is updated and taken back, one never seen joins, one no
/// longer listed leaves, and one that left before `forget` is forgotten.
fn reconcile(
    kept: Vec<Versioned<Membership>>,
    listed: Option<Vec<Listed>>,
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

/// The member `held` polled, and its cursor written where it moved.
async fn member<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    source: &PreparedSource,
    (expand, http): (&dyn Expand, &H),
    (mut held, first): (Held, Timestamp),
    run: &str,
) -> Swept {
    let ports = flow.2;
    let swept = |outcome| Swept {
        source: source.name.clone(),
        member: Some(held.membership.repository.path.clone()),
        outcome,
    };
    let polled = if let Err(stop) = ports.budget.allows(ports.clock.now(), 1) {
        Err(stop)
    } else {
        let poll = expand.member(&held.membership.repository);
        let holder = format!(
            "{run}/{}/{}",
            source.name.as_str(),
            held.membership.repository.id
        );
        let since = (held.membership.discovered > first).then_some(held.membership.discovered);
        found(
            flow,
            &source.name,
            (poll.as_ref(), http),
            (held.membership.cursor.as_ref(), since, source.backfill),
            &holder,
        )
        .await
    };
    let outcome = match polled {
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

/// What the owner `expand` lists through `http` at `now`, conditional on
/// `known`: the members it takes and the listing to keep, or neither where
/// it is unchanged.
async fn listed(
    expand: &dyn Expand,
    http: &dyn herald_ports::DynHttp,
    known: Option<&Listing>,
    now: Timestamp,
) -> Result<(Option<Vec<Listed>>, Option<Listing>), Unlisted> {
    Ok(match expand.listed(http, known).await? {
        Expanded::Unchanged => (None, None),
        Expanded::Listed {
            members,
            owner,
            pages,
        } => (
            Some(members),
            Some(Listing {
                owner,
                pages,
                first: known.map_or(now, |known| known.first),
                listed: now,
            }),
        ),
    })
}

/// The owner `expand` lists, through `http`, reconciled with the members
/// `source` keeps, and each member it takes polled. A listing over a cap
/// refuses the source for the round; one that fails keeps the last
/// membership, polling the members it holds and adding and dropping none.
/// The listing is written only once every member it changed is, so a
/// listing the next round finds unchanged never hides a member.
pub(super) async fn swept<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    source: &PreparedSource,
    expand: &dyn Expand,
    http: &H,
    run: &str,
) -> Vec<Swept> {
    let (config, _, ports) = flow;
    let told = |outcome| Swept {
        source: source.name.clone(),
        member: None,
        outcome,
    };
    let now = ports.clock.now();
    if let Err(stop) = ports.budget.allows(now, 1) {
        return vec![told(Sweep::Stopped(stop))];
    }
    let cursors = ports.store.cursors();
    let read = async {
        Ok::<_, StoreError>((
            cursors.listing(&source.name).await?,
            cursors.members(&source.name).await?,
        ))
    };
    let (record, kept) = match read.await {
        Ok(read) => read,
        Err(error) => return vec![told(Sweep::Stopped(stored(error)))],
    };
    let known = record.as_ref().map(|record| &record.value);
    let mut swept = Vec::new();
    let (listed, listing) = match listed(expand, http, known, now).await {
        Ok(listed) => listed,
        Err(Unlisted::Over(reason)) => return vec![told(Sweep::Stopped(Stop::Source(reason)))],
        Err(Unlisted::Failed(unpolled)) => {
            swept.push(told(Sweep::Stopped(match unpolled {
                Unpolled::Unreachable(reason) => Stop::Unreachable(reason),
                Unpolled::Unreadable(reason) => Stop::Source(reason),
            })));
            (None, None)
        }
    };
    let first = listing
        .as_ref()
        .or(known)
        .map_or(now, |listing| listing.first);
    let reconciled = reconcile(kept, listed, now, now - expand.keep_members());
    let mut whole = true;
    for id in &reconciled.forgotten {
        if let Err(error) = cursors.forget_member(&source.name, id).await {
            whole = false;
            swept.push(told(Sweep::Stopped(stored(error))));
        }
    }
    let mut takers = Vec::new();
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
        if held.membership.left.is_none() {
            takers.push(held);
        }
    }
    if let Some(listing) = listing.filter(|_| whole) {
        let expected = record.as_ref().map(|record| record.version);
        if let Remembered::Failed(stop) = remembered(
            cursors.put_listing(&source.name, &listing, expected).await,
            "the owner's listing",
        ) {
            swept.push(told(Sweep::Stopped(stop)));
        }
    }
    swept.push(told(Sweep::Listed {
        members: takers.len(),
        joined: reconciled.joined,
        left: reconciled.left,
    }));
    let polled: Vec<Swept> = stream::iter(
        takers
            .into_iter()
            .map(|held| member(flow, source, (expand, http), (held, first), run)),
    )
    .buffered(at_once(ports.store, config.run.sources).get())
    .collect()
    .await;
    swept.extend(polled);
    swept
}

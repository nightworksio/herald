//! Kept mode: a view source's open pull requests read from the records the
//! store keeps, reconciled with a listing when due, and each webhook
//! delivery's change folded into them (ADR-0061).

use std::collections::{BTreeMap, BTreeSet};

use herald_config::{Config, ViewConfig, ViewMode};
use herald_core::{
    Folded, Folding, KeptPull, Pull, PullKey, Reconciled, RepositoryKey, SourceName, Visibility,
};
use herald_kinds::{Asked, Expand, Queue, Relisted, Scope};
use herald_ports::{Clock, Cursors, Pulls, Signer, Store, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use crate::announce::{Ports, Stop};
use crate::budget::Halting;
use crate::poll::stored;

/// Why a view reading kept pull requests could not.
pub(crate) const UNKEPT: &str = "the view reads kept pull requests, and this store keeps none; choose a store that keeps them, or set the view's `mode` to \"live\"";

/// How many times a change is folded again where another write moved its
/// record first.
const ATTEMPTS: usize = 3;

/// Whether `source` of `config` takes webhooks, its own or its GitHub
/// App's.
pub(crate) fn webhooked(config: &Config, source: &SourceName) -> bool {
    config
        .sources
        .get(source)
        .is_some_and(|source| source.webhook().is_some() || source.installed().is_some())
}

/// Whether `view` reads `source` kept: always where it says kept, never
/// where it says live, and else where the store `keeps` pull requests and
/// the source takes webhooks.
pub(crate) fn reads_kept(
    (config, keeps): (&Config, bool),
    view: &ViewConfig,
    source: &SourceName,
) -> bool {
    match view.mode {
        ViewMode::Live => false,
        ViewMode::Kept => true,
        ViewMode::Auto => keeps && webhooked(config, source),
    }
}

/// Whether a view reads `source` kept from `store`, so its deliveries'
/// changes are folded.
pub(crate) fn folds<S: Store>((config, store): (&Config, &S), source: &SourceName) -> bool {
    let keeps = store.pulls().is_some();
    config
        .views
        .values()
        .any(|view| view.sources.contains(source) && reads_kept((config, keeps), view, source))
}

/// The open pull requests of `source` that `view` reads kept, as `asked`,
/// the most recently active first and at most `view.most.open`: its listing
/// reconciled first where due, each changed pull request read in full
/// through `queue` and `http`, at most `view.most.pulls` where `detailed`
/// says each takes requests of its own. An owner source's are those of the
/// repositories `asked.scope` holds, each seen as its membership record
/// says where that is stricter (ADR-0062).
pub(crate) async fn kept_pulls<S: Store, C: Clock, G: Signer, H: Halting>(
    ports: &Ports<'_, S, C, G>,
    (source, queue, http): (&SourceName, &dyn Queue, &H),
    (view, asked, detailed): (&ViewConfig, Asked<'_>, bool),
) -> Result<Vec<Pull>, Stop> {
    let pulls = ports
        .store
        .pulls()
        .ok_or_else(|| Stop::Store(UNKEPT.to_owned()))?;
    let now = ports.clock.now();
    let mark = pulls.reconciled(source).await.map_err(stored)?;
    let held = if Reconciled::due(mark.as_ref().map(|mark| &mark.value), view.reconcile, now) {
        let reconciling = Reconciling {
            pulls,
            queue,
            http,
            asked,
            detailed,
            now,
        };
        reconciling.reconciled(source, mark).await?
    } else {
        mark.map(|mark| mark.value).unwrap_or_default()
    };
    let mut open = Vec::new();
    for (repository, seen) in held_repositories(&held, asked.scope) {
        let kept = pulls.pulls_of(&repository).await.map_err(stored)?;
        open.extend(
            kept.into_iter()
                .filter(|kept| kept.value.open())
                .map(|kept| narrowed(kept.value, seen).pull),
        );
    }
    open.sort_by_key(|pull| std::cmp::Reverse(pull.updated));
    open.truncate(view.most.open);
    Ok(open)
}

/// The repositories `held` names that a view reads, each with who its
/// membership record says may see it where it is an owner's member: its
/// one repository, or each of the owner's that `scope` still holds.
fn held_repositories(
    held: &Reconciled,
    scope: Option<Scope<'_>>,
) -> Vec<(RepositoryKey, Option<Visibility>)> {
    match scope {
        None => held
            .repository
            .clone()
            .map(|key| (key, None))
            .into_iter()
            .collect(),
        Some(scope) => held
            .repositories
            .iter()
            .filter_map(|key| {
                let member = scope.members.iter().find(|member| member.id == key.id)?;
                Some((key.clone(), Some(member.visibility)))
            })
            .collect(),
    }
}

/// One source's reconcile.
struct Reconciling<'a, P, H> {
    pulls: &'a P,
    queue: &'a dyn Queue,
    http: &'a H,
    asked: Asked<'a>,
    detailed: bool,
    now: Timestamp,
}

impl<P: Pulls, H: Halting> Reconciling<'_, P, H> {
    /// Where the reconcile of `source` stands once its listing, conditional
    /// on `mark`, is folded into its records: each listed one it keeps none
    /// of or whose record is stale read in full, each kept one still
    /// running its checks read again where nothing changed, and each kept
    /// open one no longer listed closed where the listing was whole; an
    /// owner's repository its scope no longer holds has each of its records
    /// closed and is forgotten.
    async fn reconciled(
        &self,
        source: &SourceName,
        mark: Option<Versioned<Reconciled>>,
    ) -> Result<Reconciled, Stop> {
        let before = mark
            .as_ref()
            .map(|mark| mark.value.clone())
            .unwrap_or_default();
        let relisted = self
            .queue
            .listed(self.http, self.asked, &before.validators)
            .await
            .map_err(|unpolled| self.http.halted(unpolled))?;
        let written = match (relisted, self.asked.scope) {
            (Relisted::Unchanged, scope) => {
                let settling = held_repositories(&before, scope)
                    .into_iter()
                    .map(|(repository, seen)| (repository, None, seen))
                    .collect();
                self.settled(settling).await?;
                Reconciled {
                    at: Some(self.now),
                    ..before
                }
            }
            (
                Relisted::Listed {
                    pulls,
                    repository,
                    visibility,
                    validators,
                },
                None,
            ) => {
                let whole = pulls.len() < self.asked.most.open;
                let listed = repository
                    .zip(pulls.first())
                    .and_then(|(id, pull)| RepositoryKey::of(&pull.url, &id))
                    .or(before.repository);
                if let Some(repository) = &listed {
                    self.settled(vec![(repository.clone(), Some((pulls, whole)), visibility)])
                        .await?;
                }
                Reconciled {
                    repository: listed,
                    repositories: before.repositories,
                    at: Some(self.now),
                    validators,
                }
            }
            (
                Relisted::Listed {
                    pulls, validators, ..
                },
                Some(scope),
            ) => {
                let held = self.scoped(&before, scope, pulls).await?;
                Reconciled {
                    repository: before.repository,
                    repositories: held,
                    at: Some(self.now),
                    validators,
                }
            }
        };
        self.pulls
            .put_reconciled(source, &written, mark.map(|mark| mark.version))
            .await
            .map_err(stored)?;
        Ok(written)
    }

    /// The repositories of `scope` once `listed`, the open pull requests a
    /// listing over them read, is folded into their records, each seen as
    /// its membership record says where that is stricter; those `before`
    /// kept that the scope no longer holds have their records closed.
    async fn scoped(
        &self,
        before: &Reconciled,
        scope: Scope<'_>,
        listed: Vec<Pull>,
    ) -> Result<BTreeSet<RepositoryKey>, Stop> {
        let whole = listed.len() < self.asked.most.open;
        let mut grouped: BTreeMap<RepositoryKey, (Vec<Pull>, Option<Visibility>)> = BTreeMap::new();
        let mut left = Vec::new();
        for key in &before.repositories {
            match scope.members.iter().find(|member| member.id == key.id) {
                Some(member) => {
                    grouped.insert(key.clone(), (Vec::new(), Some(member.visibility)));
                }
                None => left.push(key.clone()),
            }
        }
        let keyed = listed.into_iter().filter_map(|pull| {
            let member = scope
                .members
                .iter()
                .find(|member| member.path.eq_ignore_ascii_case(&pull.repository))?;
            let key = RepositoryKey::of(&pull.url, &member.id)?;
            Some((key, member.visibility, pull))
        });
        for (key, seen, pull) in keyed {
            grouped
                .entry(key)
                .or_insert_with(|| (Vec::new(), Some(seen)))
                .0
                .push(pull);
        }
        let held = grouped.keys().cloned().collect();
        let settling = grouped
            .into_iter()
            .map(|(key, (pulls, seen))| (key, Some((pulls, whole)), seen))
            .chain(
                left.into_iter()
                    .map(|key| (key, Some((Vec::new(), true)), None)),
            )
            .collect();
        self.settled(settling).await?;
        Ok(held)
    }

    /// The records of each repository of `settling` brought to what was
    /// listed of it, its open pull requests as a listing reads them and
    /// whether that listing was whole, or to what they say where nothing
    /// changed; each record, read or not, narrowed to who the listing or a
    /// membership record says may see the repository, where one says. A
    /// kept open one a whole listing no longer lists is closed. Those that
    /// changed are read in full together, at most `most.pulls` where each
    /// takes requests of its own.
    async fn settled(&self, settling: Vec<Settling>) -> Result<(), Stop> {
        let mut planned = Vec::with_capacity(settling.len());
        let mut stale = Vec::new();
        let mut owners: Vec<((String, u64), usize)> = Vec::new();
        for (place, (repository, listed, seen)) in settling.into_iter().enumerate() {
            let (kept, changed, vanished) = self.planned(&repository, listed).await?;
            owners.extend(
                changed
                    .iter()
                    .map(|pull| ((pull.repository.clone(), pull.number), place)),
            );
            stale.extend(changed);
            planned.push((repository, kept, vanished, seen));
        }
        let most = if self.detailed {
            self.asked.most.pulls
        } else {
            stale.len()
        };
        let mut read = stale;
        let unread = read.split_off(most.min(read.len()));
        let read = self
            .queue
            .completed(self.http, self.asked, read)
            .await
            .map_err(|unpolled| self.http.halted(unpolled))?;
        let mut placed: BTreeMap<(String, u64), Vec<(Pull, bool)>> = BTreeMap::new();
        for (pull, read) in read
            .into_iter()
            .map(|pull| (pull, true))
            .chain(unread.into_iter().map(|pull| (pull, false)))
        {
            placed
                .entry((pull.repository.clone(), pull.number))
                .or_default()
                .push((pull, read));
        }
        for (place, (repository, mut kept, vanished, seen)) in planned.into_iter().enumerate() {
            let mine: Vec<(Pull, bool)> = owners
                .iter()
                .filter(|(_, owner)| *owner == place)
                .filter_map(|(at, _)| placed.remove(at))
                .flatten()
                .collect();
            for (pull, read) in mine {
                if !read && kept.contains_key(&pull.number) {
                    continue;
                }
                let before = kept.remove(&pull.number);
                let pull = if read {
                    pull
                } else {
                    Pull {
                        unread: true,
                        ..pull
                    }
                };
                let version = before.as_ref().map(|before| before.version);
                let key = PullKey {
                    repository: repository.clone(),
                    number: pull.number,
                };
                let record = KeptPull::read(
                    before.as_ref().map(|before| &before.value),
                    key,
                    pull,
                    self.now,
                );
                self.write(&narrowed(record, seen), version).await?;
            }
            for before in vanished {
                let record = before.value.vanished(self.now);
                self.write(&narrowed(record, seen), Some(before.version))
                    .await?;
            }
            for before in kept.into_values() {
                let record = narrowed(before.value.clone(), seen);
                if record != before.value {
                    self.write(&record, Some(before.version)).await?;
                }
            }
        }
        Ok(())
    }

    /// The records of `repository`, the pull requests `listed` names whose
    /// records are stale or that it keeps none of, or those still running
    /// their checks where nothing was listed, and the numbers of those kept
    /// open that a whole listing no longer lists.
    async fn planned(
        &self,
        repository: &RepositoryKey,
        listed: Option<(Vec<Pull>, bool)>,
    ) -> Result<Planned, Stop> {
        let mut kept: BTreeMap<u64, Versioned<KeptPull>> = self
            .pulls
            .pulls_of(repository)
            .await
            .map_err(stored)?
            .into_iter()
            .map(|kept| (kept.value.key.number, kept))
            .collect();
        let Some((listed, whole)) = listed else {
            let running = kept
                .values()
                .filter(|kept| kept.value.open() && self.rereads(&kept.value))
                .map(|kept| kept.value.pull.clone())
                .collect();
            return Ok((kept, running, Vec::new()));
        };
        let numbers: Vec<u64> = listed.iter().map(|pull| pull.number).collect();
        let gone: Vec<u64> = kept
            .iter()
            .filter(|(number, kept)| whole && kept.value.open() && !numbers.contains(number))
            .map(|(number, _)| *number)
            .collect();
        let vanished = gone
            .iter()
            .filter_map(|number| kept.remove(number))
            .collect();
        let stale = listed
            .into_iter()
            .filter(|pull| {
                kept.get(&pull.number)
                    .is_none_or(|kept| kept.value.stale(pull) || self.unread_files(&kept.value))
            })
            .collect();
        Ok((kept, stale, vanished))
    }

    /// Whether a kept open record is read in full again where nothing
    /// changed: its checks still run, or the view predicts conflicts and its
    /// head's changed paths were never read (ADR-0063).
    fn rereads(&self, kept: &KeptPull) -> bool {
        kept.pull.checks == herald_core::Checks::Pending || self.unread_files(kept)
    }

    /// Whether the view predicts conflicts and a kept record's head's
    /// changed paths were never read.
    const fn unread_files(&self, kept: &KeptPull) -> bool {
        self.asked.files && kept.pull.files.is_none()
    }

    /// `record` written over `version`; a write another run or delivery
    /// moved first is left to the next reconcile.
    async fn write(&self, record: &KeptPull, version: Option<Version>) -> Result<(), Stop> {
        self.pulls.put_pull(record, version).await.map_err(stored)?;
        Ok(())
    }
}

/// A repository whose records a reconcile settles: what its listing found
/// of it and whether that listing was whole, where it found anything, and
/// who may see it, where the listing or a membership record says.
type Settling = (RepositoryKey, Option<(Vec<Pull>, bool)>, Option<Visibility>);

/// A repository's records as a reconcile found them, the pull requests of
/// it to read in full, and the records to close, taken out of the rest.
type Planned = (
    BTreeMap<u64, Versioned<KeptPull>>,
    Vec<Pull>,
    Vec<Versioned<KeptPull>>,
);

/// `record`, seen by `seen` where that is stricter: what a listing says
/// of who may see a repository narrows each of its records, and never
/// widens one.
fn narrowed(record: KeptPull, seen: Option<Visibility>) -> KeptPull {
    match seen {
        Some(seen) => record.narrowed(seen),
        None => record,
    }
}

/// `folding`, a webhook delivery's change of the pull requests of one of
/// `source`'s repositories, folded into the records `store` keeps, and the
/// repository kept as the source's where it names none yet: how many
/// records it wrote. An owner source, which `owned` names its members'
/// projects for, folds only a delivery of a member its last membership
/// record holds and that has not left, each pull request named as the
/// member's (ADR-0062). A store keeping no pull requests folds nothing.
///
/// # Errors
///
/// Where the store could not be read or written, or other writes moved a
/// record first each time it was folded.
pub(crate) async fn fold<S: Store>(
    store: &S,
    (source, owned): (&SourceName, Option<&dyn Expand>),
    folding: &Folding,
) -> Result<usize, StoreError> {
    let Some(pulls) = store.pulls() else {
        return Ok(0);
    };
    let repository = &folding.repository;
    let mut folded = folding.folded.clone();
    if let Some(owned) = owned {
        let Some(member) = store.cursors().member(source, &repository.id).await? else {
            return Ok(0);
        };
        if member.value.left.is_some() {
            return Ok(0);
        }
        let project = owned
            .project(&member.value.repository)
            .unwrap_or(member.value.repository.name.clone());
        for said in &mut folded {
            if let Folded::Pull(described) = said {
                described.project.clone_from(&project);
            }
        }
    }
    let mark = pulls.reconciled(source).await?;
    let before = mark
        .as_ref()
        .map(|mark| mark.value.clone())
        .unwrap_or_default();
    let named = if owned.is_some() {
        let mut held = before.repositories.clone();
        held.insert(repository.clone()).then_some(Reconciled {
            repositories: held,
            ..before
        })
    } else {
        before.repository.is_none().then(|| Reconciled {
            repository: Some(repository.clone()),
            ..before
        })
    };
    if let Some(named) = named {
        pulls
            .put_reconciled(source, &named, mark.map(|mark| mark.version))
            .await?;
    }
    let mut written = 0;
    for folded in &folded {
        if let Some(number) = folded.number() {
            let key = PullKey {
                repository: repository.clone(),
                number,
            };
            written += usize::from(folded_once(pulls, &key, folded).await?);
        } else {
            for kept in pulls.pulls_of(repository).await? {
                written += usize::from(folded_once(pulls, &kept.value.key, folded).await?);
            }
        }
    }
    Ok(written)
}

/// The record keyed `key` with `folded` taken in, written where it
/// changed: whether it was.
async fn folded_once<P: Pulls>(
    pulls: &P,
    key: &PullKey,
    folded: &Folded,
) -> Result<bool, StoreError> {
    for _ in 0..ATTEMPTS {
        let kept = pulls.pull(key).await?;
        let Some(record) = KeptPull::fold(
            kept.as_ref().map(|kept| kept.value.clone()),
            key.clone(),
            folded,
        ) else {
            return Ok(false);
        };
        if kept.as_ref().is_some_and(|kept| kept.value == record) {
            return Ok(false);
        }
        let written = pulls
            .put_pull(&record, kept.map(|kept| kept.version))
            .await?;
        if matches!(written, Written::Written(_)) {
            return Ok(true);
        }
    }
    Err(StoreError::unavailable(format_args!(
        "other writes moved the record of pull request {key} first {ATTEMPTS} times"
    )))
}

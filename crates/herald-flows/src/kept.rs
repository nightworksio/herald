//! Kept mode: a view source's open pull requests read from the records the
//! store keeps, reconciled with a listing when due, and each webhook
//! delivery's change folded into them (ADR-0061).

use std::collections::BTreeMap;

use herald_config::{Config, ViewConfig, ViewMode};
use herald_core::{
    Folded, Folding, KeptPull, Pull, PullKey, Reconciled, RepositoryKey, SourceName, Visibility,
};
use herald_kinds::{Queue, Relisted};
use herald_ports::{Clock, Http, Pulls, Signer, Store, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use crate::announce::{Ports, Stop};
use crate::poll::{halted, stored};

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

/// The open pull requests of `source` that `view` reads kept, the most
/// recently active first and at most `view.most.open`: its listing
/// reconciled first where due, each changed pull request read in full
/// through `queue` and `http`, at most `view.most.pulls` where `detailed`
/// says each takes requests of its own.
pub(crate) async fn kept_pulls<S: Store, C: Clock, G: Signer, H: Http>(
    ports: &Ports<'_, S, C, G>,
    (source, queue, http): (&SourceName, &dyn Queue, &H),
    (view, detailed): (&ViewConfig, bool),
) -> Result<Vec<Pull>, Stop> {
    let pulls = ports
        .store
        .pulls()
        .ok_or_else(|| Stop::Store(UNKEPT.to_owned()))?;
    let now = ports.clock.now();
    let mark = pulls.reconciled(source).await.map_err(stored)?;
    let repository = if Reconciled::due(mark.as_ref().map(|mark| &mark.value), view.reconcile, now)
    {
        ports.budget.allows(now, 1)?;
        let reconciling = Reconciling {
            pulls,
            queue,
            http,
            view,
            detailed,
            now,
        };
        reconciling.reconciled(source, mark).await?
    } else {
        mark.and_then(|mark| mark.value.repository)
    };
    let Some(repository) = repository else {
        return Ok(Vec::new());
    };
    let mut open: Vec<Pull> = pulls
        .pulls_of(&repository)
        .await
        .map_err(stored)?
        .into_iter()
        .filter(|kept| kept.value.open())
        .map(|kept| kept.value.pull)
        .collect();
    open.sort_by_key(|pull| std::cmp::Reverse(pull.updated));
    open.truncate(view.most.open);
    Ok(open)
}

/// One source's reconcile.
struct Reconciling<'a, P, H> {
    pulls: &'a P,
    queue: &'a dyn Queue,
    http: &'a H,
    view: &'a ViewConfig,
    detailed: bool,
    now: Timestamp,
}

impl<P: Pulls, H: Http> Reconciling<'_, P, H> {
    /// The repository of `source` once its listing, conditional on `mark`,
    /// is folded into its records: each listed one it keeps none of or
    /// whose record is stale read in full, each kept one still running its
    /// checks read again where nothing changed, and each kept open one no
    /// longer listed closed; where it stands then is kept.
    async fn reconciled(
        &self,
        source: &SourceName,
        mark: Option<Versioned<Reconciled>>,
    ) -> Result<Option<RepositoryKey>, Stop> {
        let kept_validators = mark
            .as_ref()
            .map(|mark| mark.value.validators.clone())
            .unwrap_or_default();
        let known = mark.as_ref().and_then(|mark| mark.value.repository.clone());
        let relisted = self
            .queue
            .listed(self.http, self.view.most, &kept_validators)
            .await
            .map_err(halted)?;
        let (repository, validators) = match relisted {
            Relisted::Unchanged => {
                if let Some(repository) = &known {
                    self.settled(repository, None, None).await?;
                }
                (known, kept_validators)
            }
            Relisted::Listed {
                pulls,
                repository,
                visibility,
                validators,
            } => {
                let listed = repository
                    .zip(pulls.first())
                    .and_then(|(id, pull)| RepositoryKey::of(&pull.url, &id))
                    .or(known);
                if let Some(repository) = &listed {
                    self.settled(repository, Some(pulls), visibility).await?;
                }
                (listed, validators)
            }
        };
        let written = Reconciled {
            repository: repository.clone(),
            at: Some(self.now),
            validators,
        };
        self.pulls
            .put_reconciled(source, &written, mark.map(|mark| mark.version))
            .await
            .map_err(stored)?;
        Ok(repository)
    }

    /// The records of `repository` brought to `listed`, its open pull
    /// requests as a listing reads them, or to what they say where nothing
    /// changed; each record, read or not, narrowed to `seen`, who the
    /// listing says may see the repository, where it says.
    async fn settled(
        &self,
        repository: &RepositoryKey,
        listed: Option<Vec<Pull>>,
        seen: Option<Visibility>,
    ) -> Result<(), Stop> {
        let mut kept: BTreeMap<u64, Versioned<KeptPull>> = self
            .pulls
            .pulls_of(repository)
            .await
            .map_err(stored)?
            .into_iter()
            .map(|kept| (kept.value.key.number, kept))
            .collect();
        let (stale, vanished): (Vec<Pull>, Vec<Versioned<KeptPull>>) = if let Some(listed) = listed
        {
            let numbers: Vec<u64> = listed.iter().map(|pull| pull.number).collect();
            let gone: Vec<u64> = kept
                .iter()
                .filter(|(number, kept)| kept.value.open() && !numbers.contains(number))
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
                        .is_none_or(|kept| kept.value.stale(pull))
                })
                .collect();
            (stale, vanished)
        } else {
            let running = kept
                .values()
                .filter(|kept| {
                    kept.value.open() && kept.value.pull.checks == herald_core::Checks::Pending
                })
                .map(|kept| kept.value.pull.clone())
                .collect();
            (running, Vec::new())
        };
        let most = if self.detailed {
            self.view.most.pulls
        } else {
            stale.len()
        };
        let mut read = stale;
        let unread = read.split_off(most.min(read.len()));
        let read = self
            .queue
            .completed(self.http, read)
            .await
            .map_err(halted)?;
        let unread = unread
            .into_iter()
            .filter(|pull| !kept.contains_key(&pull.number))
            .map(|pull| Pull {
                unread: true,
                ..pull
            })
            .collect::<Vec<Pull>>();
        for pull in read.into_iter().chain(unread) {
            let key = PullKey {
                repository: repository.clone(),
                number: pull.number,
            };
            let before = kept.remove(&pull.number);
            let version = before.as_ref().map(|before| before.version);
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
        Ok(())
    }

    /// `record` written over `version`; a write another run or delivery
    /// moved first is left to the next reconcile.
    async fn write(&self, record: &KeptPull, version: Option<Version>) -> Result<(), Stop> {
        self.pulls.put_pull(record, version).await.map_err(stored)?;
        Ok(())
    }
}

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
/// records it wrote. A store keeping no pull requests folds nothing.
///
/// # Errors
///
/// Where the store could not be read or written, or other writes moved a
/// record first each time it was folded.
pub(crate) async fn fold<S: Store>(
    store: &S,
    source: &SourceName,
    folding: &Folding,
) -> Result<usize, StoreError> {
    let Some(pulls) = store.pulls() else {
        return Ok(0);
    };
    let repository = &folding.repository;
    let mark = pulls.reconciled(source).await?;
    if mark
        .as_ref()
        .is_none_or(|mark| mark.value.repository.is_none())
    {
        let named = Reconciled {
            repository: Some(repository.clone()),
            ..mark.as_ref().map_or(
                Reconciled {
                    repository: None,
                    at: None,
                    validators: Vec::new(),
                },
                |mark| mark.value.clone(),
            )
        };
        pulls
            .put_reconciled(source, &named, mark.map(|mark| mark.version))
            .await?;
    }
    let mut written = 0;
    for folded in &folding.folded {
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

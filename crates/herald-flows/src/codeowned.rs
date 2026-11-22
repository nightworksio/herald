//! The areas a pull request falls in by its source: its table's path
//! globs, or its repository's CODEOWNERS file, read once a round per
//! repository and branch, each GitLab top-level `@name` it writes looked
//! up once (ADR-0071, ADR-0073). Where the store keeps pull requests, each
//! file read is kept with its forge's validator, and a file unchanged
//! since is neither downloaded nor read again (ADR-0074).

use std::collections::{BTreeMap, BTreeSet};

use herald_core::{
    Area, AreaKind, Changed, CodeOwner, CodeownersKey, CodeownersRef, KeptCodeowners,
    MOST_CODEOWNERS_LOOKUPS, Pull, SourceName, Zoning,
};
use herald_kinds::{Codeowning, Found, Queue, Source, Unpolled};
use herald_ports::{DynHttp, Pulls};
use jiff::Timestamp;

/// Why a source's CODEOWNERS file is not read where it reads no queue.
const UNQUEUED: &str = "the source reads no queue of pull requests to read CODEOWNERS through";

/// A source's queue and the HTTP it is asked through.
pub(crate) type Reach<'a> = (&'a dyn Queue, &'a dyn DynHttp);

/// The one source whose areas are read: its name, its settings, and its
/// queue with the HTTP it is asked through where it reads one.
pub(crate) type Reading<'s> = (&'s SourceName, &'s Source, Option<Reach<'s>>);

/// Where a source's CODEOWNERS reads are kept between rounds: the store's
/// pull requests where it keeps them, and when the round reads.
pub(crate) type Keeping<'s, P> = (Option<&'s P>, Timestamp);

/// What one source's CODEOWNERS reads found in a round: each file by its
/// repository and branch, none where it is at none of the places looked
/// at, and the kind each GitLab `@name` was looked up as on that source's
/// forge.
pub(crate) struct Codeowned<'s, P> {
    reading: Reading<'s>,
    keeping: Keeping<'s, P>,
    files: BTreeMap<(String, Option<String>), Option<Found>>,
    names: BTreeMap<String, AreaKind>,
}

/// A pull request as its areas are read: its repository's full path, the
/// branch it merges into where known, and the paths its head changes where
/// read.
pub(crate) type Placed<'a> = (&'a str, Option<&'a str>, Option<&'a Changed>);

impl<'s, P: Pulls> Codeowned<'s, P> {
    /// A round's reads of the one source `reading` names, kept as
    /// `keeping` says.
    pub(crate) fn new(reading: Reading<'s>, keeping: Keeping<'s, P>) -> Self {
        Self {
            reading,
            keeping,
            files: BTreeMap::new(),
            names: BTreeMap::new(),
        }
    }

    /// The areas the pull request `placed` falls in by the source, reading
    /// its repository's CODEOWNERS file through the source's queue where
    /// the source reads areas from it: none where the source names no
    /// areas, the file is at none of the places looked at, or it is
    /// refused.
    ///
    /// # Errors
    ///
    /// Where the file is to be read and could not be.
    pub(crate) async fn areas(
        &mut self,
        (repository, base, changed): Placed<'_>,
    ) -> Result<Vec<Area>, Unpolled> {
        let (_, source, reach) = self.reading;
        let Some(codeowning) = source.codeowning(repository) else {
            return Ok(match source.areas(repository) {
                Some(Zoning::Paths(areas)) => areas.of_changed(changed),
                Some(Zoning::Codeowners) | None => Vec::new(),
            });
        };
        let (queue, http) = reach.ok_or_else(|| Unpolled::Unreadable(UNQUEUED.to_owned()))?;
        let reference = match codeowning.reference {
            CodeownersRef::Base => base,
            CodeownersRef::Default => None,
        };
        let key = (repository.to_lowercase(), reference.map(str::to_owned));
        if !self.files.contains_key(&key) {
            let found = self
                .read((queue, http), (repository, reference), &codeowning)
                .await?;
            self.files.insert(key.clone(), found);
        }
        let Self { files, names, .. } = self;
        let Some(found) = files.get(&key).and_then(Option::as_ref) else {
            return Ok(Vec::new());
        };
        let owned = found.file.owning(changed);
        let looked: BTreeSet<&str> = found
            .file
            .undecided()
            .into_iter()
            .take(MOST_CODEOWNERS_LOOKUPS)
            .collect();
        for owner in &owned.owners {
            let CodeOwner::Either(name) = owner else {
                continue;
            };
            if looked.contains(name.as_str()) && !names.contains_key(name) {
                let kind = queue.named(http, name).await.unwrap_or(AreaKind::Name);
                names.insert(name.clone(), kind);
            }
        }
        Ok(found.file.areas(&owned, |name| names.get(name).copied()))
    }

    /// Each of `pulls` with the areas it falls in by the source.
    ///
    /// # Errors
    ///
    /// Where a file is to be read and could not be.
    pub(crate) async fn zone(mut self, pulls: &mut [Pull]) -> Result<(), Unpolled> {
        for pull in pulls {
            let placed = (
                pull.repository.as_str(),
                pull.base.as_deref(),
                pull.files.as_ref(),
            );
            pull.areas = self.areas(placed).await?;
        }
        Ok(())
    }
}

impl<P: Pulls> Codeowned<'_, P> {
    /// The CODEOWNERS file of the repository at `read.0` at the branch
    /// `read.1`, read through `reach` as `codeowning` says, asked
    /// conditionally where the store keeps the file: a file read in full
    /// with a validator is kept for the next round, and a store that
    /// cannot read or write it leaves the file read in full.
    async fn read(
        &self,
        (queue, http): Reach<'_>,
        (repository, reference): (&str, Option<&str>),
        codeowning: &Codeowning,
    ) -> Result<Option<Found>, Unpolled> {
        let (pulls, at) = self.keeping;
        let (source, ..) = self.reading;
        let key = CodeownersKey {
            source: source.clone(),
            repository: repository.to_lowercase(),
            reference: reference.map(str::to_owned),
        };
        let kept = match pulls {
            Some(pulls) => pulls.codeowners(&key).await.ok().flatten(),
            None => None,
        };
        let found = queue
            .codeowners(
                http,
                (repository, reference),
                (codeowning, kept.as_ref().map(|kept| &kept.value)),
            )
            .await?;
        let keeping = pulls.zip(found.as_ref()).and_then(|(pulls, found)| {
            let etag = found.etag.as_deref()?;
            let read = (codeowning.bounds(), at);
            Some((
                pulls,
                KeptCodeowners::of(key, (&found.path, etag), &found.file, read)?,
            ))
        });
        if let Some((pulls, record)) = keeping {
            pulls
                .put_codeowners(&record, kept.map(|kept| kept.version))
                .await
                .ok();
        }
        Ok(found)
    }
}

/// The names of `areas`, each once, in name order.
pub(crate) fn names(areas: Vec<Area>) -> Vec<String> {
    let mut names: Vec<String> = areas.into_iter().map(|area| area.name).collect();
    names.dedup();
    names
}

#[cfg(test)]
mod tests;

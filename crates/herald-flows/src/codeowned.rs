//! The areas a pull request falls in by its source: its table's path
//! globs, or its repository's CODEOWNERS file, read once a round per
//! repository and branch, each GitLab top-level `@name` it writes looked
//! up once (ADR-0071, ADR-0073).

use std::collections::{BTreeMap, BTreeSet};

use herald_core::{
    Area, AreaKind, Changed, CodeOwner, CodeownersRef, MOST_CODEOWNERS_LOOKUPS, Zoning,
};
use herald_kinds::{Found, Queue, Source, Unpolled};
use herald_ports::DynHttp;

/// Why a source's CODEOWNERS file is not read where it reads no queue.
const UNQUEUED: &str = "the source reads no queue of pull requests to read CODEOWNERS through";

/// A source's queue and the HTTP it is asked through.
pub(crate) type Reach<'a> = (&'a dyn Queue, &'a dyn DynHttp);

/// What one source's CODEOWNERS reads found in a round: each file by its
/// repository and branch, none where it is at none of the places looked
/// at, and the kind each GitLab `@name` was looked up as.
#[derive(Debug, Default)]
pub(crate) struct Codeowned {
    files: BTreeMap<(String, Option<String>), Option<Found>>,
    names: BTreeMap<String, AreaKind>,
}

/// A pull request as its areas are read: its repository's full path, the
/// branch it merges into where known, and the paths its head changes where
/// read.
pub(crate) type Placed<'a> = (&'a str, Option<&'a str>, Option<&'a Changed>);

impl Codeowned {
    /// The areas the pull request `placed` falls in by `source`, reading
    /// its repository's CODEOWNERS file through `reach` where the source
    /// reads areas from it: none where the source names no areas, the
    /// file is at none of the places looked at, or it is refused.
    ///
    /// # Errors
    ///
    /// Where the file is to be read and could not be.
    pub(crate) async fn areas(
        &mut self,
        (source, reach): (&Source, Option<Reach<'_>>),
        (repository, base, changed): Placed<'_>,
    ) -> Result<Vec<Area>, Unpolled> {
        let codeowning = match source.areas(repository) {
            None => return Ok(Vec::new()),
            Some(Zoning::Paths(areas)) => return Ok(areas.of_changed(changed)),
            Some(Zoning::Codeowners) => source.codeowning(repository),
        };
        let Some(codeowning) = codeowning else {
            return Ok(Vec::new());
        };
        let (queue, http) = reach.ok_or_else(|| Unpolled::Unreadable(UNQUEUED.to_owned()))?;
        let reference = match codeowning.reference {
            CodeownersRef::Base => base,
            CodeownersRef::Default => None,
        };
        let key = (repository.to_lowercase(), reference.map(str::to_owned));
        if !self.files.contains_key(&key) {
            let found = queue
                .codeowners(http, (repository, reference), &codeowning)
                .await?;
            self.files.insert(key.clone(), found);
        }
        let Self { files, names } = self;
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
}

/// The names of `areas`, each once, in name order.
pub(crate) fn names(areas: Vec<Area>) -> Vec<String> {
    let mut names: Vec<String> = areas.into_iter().map(|area| area.name).collect();
    names.dedup();
    names
}

#[cfg(test)]
mod tests;

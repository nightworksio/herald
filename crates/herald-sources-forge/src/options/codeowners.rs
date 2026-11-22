//! How a forge source reads its repositories' CODEOWNERS files, and what
//! doctor says of it (ADR-0073).

use std::collections::BTreeMap;

use herald_core::{
    CODEOWNERS, CodeownersRef, MOST_AREAS, MOST_AREAS_READ, MOST_CODEOWNERS,
    MOST_CODEOWNERS_LOOKUPS, MOST_CODEOWNERS_READ, Zoning,
};
use herald_kinds::Codeowning;
use schemars::JsonSchema;

use super::{SourceOptions, sets_codeowners, sets_codeowners_ref};
use crate::Forge;

/// The most places a source looks for a CODEOWNERS file: each is one
/// request a round where the places before it hold none.
pub const MOST_CODEOWNERS_PATHS: usize = 8;

/// The most characters of one place a CODEOWNERS file is looked for.
pub const MOST_CODEOWNERS_PATH: usize = 256;

/// A source's `areas` as the schema shows it.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code, reason = "read only by the schema's derive")]
pub(super) enum Zones {
    /// Each area's name and the path globs a path falls in it by.
    Table(BTreeMap<String, Vec<String>>),
    /// Each repository's CODEOWNERS file names its areas.
    Read(Read),
}

/// The word naming CODEOWNERS as where areas come from.
#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code, reason = "read only by the schema's derive")]
pub(super) enum Read {
    /// Areas read from each repository's CODEOWNERS file.
    Codeowners,
}

impl Forge {
    /// The places the forge itself looks for a CODEOWNERS file, in its own
    /// order.
    #[must_use]
    pub const fn codeowners(self) -> &'static [&'static str] {
        match self {
            Self::GitHub => &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"],
            Self::GitLab => &["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"],
            Self::Forgejo => &[
                "CODEOWNERS",
                "docs/CODEOWNERS",
                ".gitea/CODEOWNERS",
                ".forgejo/CODEOWNERS",
            ],
        }
    }

    /// The branch the forge itself reads a pull request's CODEOWNERS file
    /// at: Forgejo its repository's default branch, the others the branch
    /// it merges into.
    #[must_use]
    pub const fn codeowners_ref(self) -> CodeownersRef {
        match self {
            Self::GitHub | Self::GitLab => CodeownersRef::Base,
            Self::Forgejo => CodeownersRef::Default,
        }
    }
}

/// Why `path` is no place in a repository, where it is not one.
fn unplaced(path: &str) -> Option<String> {
    if path.chars().count() > MOST_CODEOWNERS_PATH {
        return Some(format!(
            "\"{path}\" is longer than {MOST_CODEOWNERS_PATH} characters; shorten it"
        ));
    }
    let placed = path
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
        && !path.chars().any(char::is_control);
    (!placed).then(|| {
        format!(
            "\"{}\" is no path in a repository; give one such as \"docs/CODEOWNERS\", with no leading /, no . or .. and no empty part",
            path.escape_debug()
        )
    })
}

/// Why the places `paths`, given at `key`, are not read, where they are
/// not.
fn paths_check(key: &str, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() || paths.len() > MOST_CODEOWNERS_PATHS {
        return Err(format!(
            "{key} names {} places, and 1 to {MOST_CODEOWNERS_PATHS} are read",
            paths.len()
        ));
    }
    paths
        .iter()
        .find_map(|path| unplaced(path))
        .map_or(Ok(()), |reason| Err(format!("{key}: {reason}")))
}

/// How `reference` is said.
const fn said(reference: CodeownersRef) -> &'static str {
    match reference {
        CodeownersRef::Base => "the branch each pull request merges into",
        CodeownersRef::Default => "the repository's default branch",
    }
}

impl SourceOptions {
    /// Whether it or one of its overrides reads areas from CODEOWNERS.
    #[must_use]
    pub fn codeowned(&self) -> bool {
        self.areas
            .iter()
            .chain(
                self.repositories
                    .values()
                    .filter_map(|chosen| chosen.areas.as_ref()),
            )
            .any(|zoning| *zoning == Zoning::Codeowners)
    }

    /// The most bytes of a CODEOWNERS file it reads.
    #[must_use]
    pub fn most_codeowners(&self) -> usize {
        self.most_codeowners
            .map_or(MOST_CODEOWNERS_READ, |most| most.0)
    }

    /// The most areas it reads a repository's table or CODEOWNERS file
    /// naming.
    #[must_use]
    pub fn most_areas(&self) -> usize {
        self.most_areas.unwrap_or(MOST_AREAS_READ)
    }

    /// Whether its `most_areas` is sound, and each of its tables names at
    /// most that many areas.
    fn most_areas_check(&self) -> Result<(), String> {
        if self
            .most_areas
            .is_some_and(|most| !(1..=MOST_AREAS).contains(&most))
        {
            return Err(format!(
                "most_areas is 1 to {MOST_AREAS}: each area is a group of a board and is matched against every changed path; give one in that range"
            ));
        }
        let tables = self
            .areas
            .iter()
            .map(|zoning| ("areas".to_owned(), zoning))
            .chain(self.repositories.iter().filter_map(|(glob, chosen)| {
                Some((
                    format!("repositories.\"{glob}\".areas"),
                    chosen.areas.as_ref()?,
                ))
            }));
        let mut zoned = false;
        for (key, zoning) in tables {
            zoned = true;
            let named = zoning.names().count();
            if named > self.most_areas() {
                return Err(format!(
                    "{key} names {named} areas, and the source reads at most {}; name fewer, or raise most_areas, at most {MOST_AREAS}",
                    self.most_areas()
                ));
            }
        }
        if self.most_areas.is_some() && !zoned {
            return Err(
                "most_areas is read only where areas is set; set it, or remove most_areas"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// How it reads the CODEOWNERS file of the repository named `name` on
    /// `forge`, where that repository's areas are read from one.
    #[must_use]
    pub fn codeowning_of(&self, forge: Forge, name: &str) -> Option<Codeowning> {
        if self.areas_of(name) != Some(&Zoning::Codeowners) {
            return None;
        }
        let paths = self
            .narrowest(name, sets_codeowners)
            .and_then(|chosen| chosen.codeowners.clone())
            .or_else(|| self.codeowners.clone())
            .unwrap_or_else(|| {
                forge
                    .codeowners()
                    .iter()
                    .map(|path| (*path).to_owned())
                    .collect()
            });
        let reference = self
            .narrowest(name, sets_codeowners_ref)
            .and_then(|chosen| chosen.codeowners_ref)
            .or(self.codeowners_ref)
            .unwrap_or_else(|| forge.codeowners_ref());
        Some(Codeowning {
            paths,
            reference,
            most: self.most_codeowners(),
            most_areas: self.most_areas(),
        })
    }

    /// Whether its CODEOWNERS keys are sound.
    pub(super) fn codeowners_check(&self) -> Result<(), String> {
        self.most_areas_check()?;
        if self
            .most_codeowners
            .is_some_and(|most| most.0 > MOST_CODEOWNERS)
        {
            return Err(format!(
                "most_codeowners is at most {} MiB, GitHub's own limit; lower it",
                MOST_CODEOWNERS >> 20
            ));
        }
        if let Some(paths) = &self.codeowners {
            paths_check("codeowners", paths)?;
        }
        for (glob, chosen) in &self.repositories {
            if let Some(paths) = &chosen.codeowners {
                paths_check(&format!("repositories.\"{glob}\".codeowners"), paths)?;
            }
        }
        let keyed = self.codeowners.is_some()
            || self.codeowners_ref.is_some()
            || self.most_codeowners.is_some()
            || self
                .repositories
                .values()
                .any(|chosen| chosen.codeowners.is_some() || chosen.codeowners_ref.is_some());
        if keyed && !self.codeowned() {
            return Err(format!(
                "codeowners, codeowners_ref and most_codeowners are read only where areas = \"{CODEOWNERS}\"; set it, or remove them"
            ));
        }
        Ok(())
    }

    /// What doctor says of how it reads CODEOWNERS on `forge`, where it
    /// does.
    #[must_use]
    pub fn codeowners_notes(&self, forge: Forge) -> Vec<String> {
        if !self.codeowned() {
            return Vec::new();
        }
        let paths = self.codeowners.clone().unwrap_or_else(|| {
            forge
                .codeowners()
                .iter()
                .map(|path| (*path).to_owned())
                .collect()
        });
        let reference = self
            .codeowners_ref
            .unwrap_or_else(|| forge.codeowners_ref());
        let mut notes = vec![format!(
            "reads areas from each repository's CODEOWNERS file at {}, looking at {} in order and reading the first found, at most {} bytes of it: each place one request a round per repository and branch until one is found; each team, group, role and login a line names for a changed path, and each GitLab section, is an area, a login marked a person in JSON; an email is never an area; as the names are known only as each file is read, a view's or a route's areas are taken as written, unchecked",
            said(reference),
            paths.join(", "),
            self.most_codeowners()
        )];
        notes.push(format!(
            "reads at most {} areas a CODEOWNERS file names, refusing a file naming more whole, which doctor names; raise most_areas, at most {MOST_AREAS}; where the store keeps pull requests, each file read is kept with its forge's validator, so one unchanged since costs a 304 answer and is neither downloaded nor read again",
            self.most_areas()
        ));
        if forge == Forge::GitLab {
            notes.push(format!(
                "looks up each top-level @name of a CODEOWNERS file once a run, a group or a person, at most {MOST_CODEOWNERS_LOOKUPS} names a file, through the source's token, API base and the run's request budget; a name past them, or one a lookup cannot tell, is an area of kind name"
            ));
        }
        if forge == Forge::Forgejo && self.codeowners_ref.is_none() {
            notes.push(
                "reads CODEOWNERS at the default branch, as Forgejo applies it; Gitea applies the pull request's base branch: on Gitea, set codeowners_ref = \"base\"".to_owned(),
            );
        }
        if self.repositories.values().any(|chosen| {
            chosen.codeowners.is_some() || chosen.codeowners_ref.is_some() || chosen.areas.is_some()
        }) {
            notes.push(
                "an override under repositories sets its own areas, codeowners or codeowners_ref for the repositories it names".to_owned(),
            );
        }
        notes
    }
}

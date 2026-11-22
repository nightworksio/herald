//! The areas of a repository a source names by path globs, and the areas
//! a pull request's changed paths fall in (ADR-0071).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Changed;
use crate::glob::Glob;

/// The most areas one table names: past it a board's groups and columns
/// stop being read, and each changed path is matched against every one.
pub const MOST_AREAS: usize = 200;

/// The most globs one table's areas hold together, each matched against
/// every changed path of every pull request read.
pub const MOST_AREA_GLOBS: usize = 1000;

/// The most characters one area glob holds: matching it against a path
/// costs its length times the path's.
pub const MOST_AREA_GLOB: usize = 256;

/// The most characters an area's name holds, as a row, a group and a
/// route's match write it.
pub const MOST_AREA_NAME: usize = 128;

/// The most characters of a changed path matched against areas; a longer
/// path, which an author chooses, falls in no area, so one path costs at
/// most this times [`MOST_AREA_GLOB`] for each glob.
pub const MOST_AREA_PATH: usize = 1024;

/// A repository's areas, each a name and the path globs any of which a
/// path falls in it by; made only by [`Areas::of`], which a config's table
/// is read through, so each name is written, every glob compiles, and the
/// table is within its bounds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(try_from = "BTreeMap<String, Vec<String>>")]
pub struct Areas {
    named: Vec<(String, Vec<Glob>)>,
}

/// What an area names, as a row's JSON marks it: a consumer tells a
/// person from a team by it (ADR-0073).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AreaKind {
    /// An area a source's table names by path globs.
    Paths,
    /// A team a CODEOWNERS line names, `@org/team`.
    Team,
    /// A GitLab group a CODEOWNERS line names.
    Group,
    /// A person a CODEOWNERS line names by login.
    Person,
    /// A GitLab role a CODEOWNERS line names, `@@developer`.
    Role,
    /// A GitLab CODEOWNERS section.
    Section,
    /// A GitLab `@name` no lookup told a group or a person.
    Name,
}

impl AreaKind {
    /// The kind as a row's JSON and a template write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Paths => "paths",
            Self::Team => "team",
            Self::Group => "group",
            Self::Person => "person",
            Self::Role => "role",
            Self::Section => "section",
            Self::Name => "name",
        }
    }
}

/// An area a pull request falls in: its name, as a view's and a route's
/// globs match it, and what it names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Area {
    /// Its name.
    pub name: String,
    /// What it names.
    pub kind: AreaKind,
}

/// How a source names a repository's areas: by its own table of path
/// globs, or by the repository's CODEOWNERS file (ADR-0071, ADR-0073).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Zoned")]
pub enum Zoning {
    /// By path globs.
    Paths(Areas),
    /// By the owners and GitLab sections of its CODEOWNERS file.
    Codeowners,
}

/// The word naming CODEOWNERS as where a source's areas come from.
pub const CODEOWNERS: &str = "codeowners";

/// A source's `areas` as written: a table of path globs, or a word.
#[derive(Deserialize)]
#[serde(untagged)]
enum Zoned {
    Word(String),
    Table(BTreeMap<String, Vec<String>>),
}

impl TryFrom<Zoned> for Zoning {
    type Error = String;

    fn try_from(written: Zoned) -> Result<Self, String> {
        match written {
            Zoned::Word(word) if word == CODEOWNERS => Ok(Self::Codeowners),
            Zoned::Word(word) => Err(format!(
                "\"{word}\" names no way to read areas; give a table of path globs, or \"{CODEOWNERS}\""
            )),
            Zoned::Table(table) => Areas::of(table).map(Self::Paths),
        }
    }
}

impl Zoning {
    /// The names its table gives, none where CODEOWNERS names them as
    /// it is read.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        match self {
            Self::Paths(areas) => Some(areas),
            Self::Codeowners => None,
        }
        .into_iter()
        .flat_map(Areas::names)
    }
}

/// Why `name` cannot name an area, where it cannot.
pub(super) fn unnamed(name: &str) -> Option<String> {
    if name.trim().is_empty() || name.trim() != name {
        Some(format!(
            "\"{name}\" is no area name; give one without spaces around it"
        ))
    } else if name.chars().count() > MOST_AREA_NAME {
        Some(format!(
            "\"{name}\" is longer than {MOST_AREA_NAME} characters; shorten it"
        ))
    } else if name.chars().any(char::is_control) {
        Some(format!(
            "\"{}\" holds a control character; remove it",
            name.escape_debug()
        ))
    } else {
        None
    }
}

impl Areas {
    /// The areas `table` names, each name with its globs.
    ///
    /// # Errors
    ///
    /// Where it names more than [`MOST_AREAS`] areas or holds more than
    /// [`MOST_AREA_GLOBS`] globs, a name is blank, padded, longer than
    /// [`MOST_AREA_NAME`] or holds a control character, an area names no
    /// glob, or a glob is longer than [`MOST_AREA_GLOB`] or cannot be read.
    pub fn of(table: BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        if table.len() > MOST_AREAS {
            return Err(format!(
                "{} areas are named, and at most {MOST_AREAS} are read; name fewer",
                table.len()
            ));
        }
        let globs: usize = table.values().map(Vec::len).sum();
        if globs > MOST_AREA_GLOBS {
            return Err(format!(
                "the areas hold {globs} globs, and at most {MOST_AREA_GLOBS} are read; give fewer"
            ));
        }
        let mut named = Vec::with_capacity(table.len());
        for (name, texts) in table {
            if let Some(reason) = unnamed(&name) {
                return Err(reason);
            }
            if texts.is_empty() {
                return Err(format!(
                    "the area \"{name}\" names no path; give it a glob such as \"{name}/**\""
                ));
            }
            let mut globs = Vec::with_capacity(texts.len());
            for text in &texts {
                if text.chars().count() > MOST_AREA_GLOB {
                    return Err(format!(
                        "the area \"{name}\" has a glob longer than {MOST_AREA_GLOB} characters; shorten it"
                    ));
                }
                globs.push(
                    Glob::new(text).map_err(|error| format!("the area \"{name}\": {error}"))?,
                );
            }
            named.push((name, globs));
        }
        Ok(Self { named })
    }

    /// Each area's name, in name order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.named.iter().map(|(name, _)| name.as_str())
    }

    /// The areas any of `changed`'s paths falls in, in name order: none
    /// where its paths were not read.
    #[must_use]
    pub fn of_changed(&self, changed: Option<&Changed>) -> Vec<Area> {
        let paths: Vec<&String> = changed
            .into_iter()
            .flat_map(|changed| &changed.paths)
            .filter(|path| path.chars().count() <= MOST_AREA_PATH)
            .collect();
        self.named
            .iter()
            .filter(|(_, globs)| {
                paths
                    .iter()
                    .any(|path| globs.iter().any(|glob| glob.matches(path)))
            })
            .map(|(name, _)| Area {
                name: name.clone(),
                kind: AreaKind::Paths,
            })
            .collect()
    }
}

impl TryFrom<BTreeMap<String, Vec<String>>> for Areas {
    type Error = String;

    fn try_from(table: BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        Self::of(table)
    }
}

/// Areas of kind paths, each of `names`.
#[cfg(test)]
pub(crate) fn of_paths(names: &[&str]) -> Vec<Area> {
    names
        .iter()
        .map(|name| Area {
            name: (*name).to_owned(),
            kind: AreaKind::Paths,
        })
        .collect()
}

#[cfg(test)]
mod tests;

//! A repository's CODEOWNERS file, read as its forge reads it, and the
//! owners and GitLab sections the paths a pull request changes fall to
//! (ADR-0073).

use std::collections::{BTreeMap, BTreeSet};

use regex::{RegexBuilder, RegexSet, RegexSetBuilder, SetMatches};
use serde::Deserialize;

use super::area::{Area, AreaKind, MOST_AREAS, unnamed};
use super::{Changed, MOST_AREA_PATH};
use crate::Size;

mod line;
mod pattern;

pub use line::CodeOwner;
use line::{heading, owners, words};

/// The most bytes of a CODEOWNERS file herald reads: GitHub's own limit,
/// past which GitHub does not read the file either.
pub const MOST_CODEOWNERS: usize = 3 * Size::MIB;

/// The most bytes of a CODEOWNERS file a source reads where its
/// `most_codeowners` is absent.
pub const MOST_CODEOWNERS_READ: usize = 128 * Size::KIB;

/// The most distinct GitLab top-level `@name`s of one file looked up as a
/// group or a person: each costs up to two requests the first time a
/// source meets it, and a name past it is an area of kind name.
pub const MOST_CODEOWNERS_LOOKUPS: usize = 50;

/// The branch a repository's CODEOWNERS file is read at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeownersRef {
    /// The branch the pull request merges into.
    Base,
    /// The repository's default branch.
    Default,
}

/// The most lines of a CODEOWNERS file herald reads: each line is matched
/// against every changed path of every pull request read.
pub const MOST_CODEOWNERS_LINES: usize = 4000;

/// The most characters one pattern holds: a path is matched only up to
/// [`MOST_AREA_PATH`] characters, and a pattern's cost grows with its
/// length.
pub const MOST_CODEOWNERS_PATTERN: usize = 1024;

/// The most owners one line names, each of them a possible area.
pub const MOST_LINE_OWNERS: usize = 100;

/// The most bytes one line's pattern compiles to: a short pattern can
/// still grow large, by a counted repetition such as `a{1000}` (about
/// 32 KiB) or a Unicode class such as `\w` (about 50 KiB, so `\w{10}` is
/// past it).
pub const MOST_RULE_COMPILED: usize = 256 * Size::KIB;

/// The most bytes a file's patterns compile to together, held while the
/// file's areas are read: a file of [`MOST_CODEOWNERS_LINES`] gitignore
/// patterns compiles to about 10 MiB.
pub const MOST_RULES_COMPILED: usize = 16 * Size::MIB;

/// The most bytes of states the lazy automaton matching a file's patterns
/// keeps, grown only as states are met: a file of
/// [`MOST_CODEOWNERS_LINES`] lines needs about this many, and matches five
/// times slower with half. Past it matching goes on by a slower engine,
/// still in time linear in the path's length.
pub const RULES_CACHE: usize = 16_777_216;

/// Which forge's CODEOWNERS a file is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// GitHub: gitignore-style patterns, the last matching line winning.
    GitHub,
    /// GitLab: `fnmatch` patterns in sections, the last matching entry of
    /// each section winning, and `!` exempting a path from its section.
    GitLab,
    /// Forgejo and Gitea: Go regular expressions, every matching line
    /// applying, and `!` applying to each path the expression does not
    /// match.
    Forgejo,
}

/// GitLab's name for the section before any heading: a heading of that
/// name, ignoring case, goes on with it, and it names no area.
const DEFAULT_SECTION: &str = "codeowners";

/// A line herald does not read, and why: it matches no path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The line's number, from 1.
    pub line: usize,
    /// Why it is not read.
    pub why: String,
}

/// The owners and named GitLab sections some paths fall to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    /// Each owner a line the paths fall to names.
    pub owners: BTreeSet<CodeOwner>,
    /// Each GitLab section one of the paths falls in whose name can name an
    /// area: never the section before any heading.
    pub sections: BTreeSet<String>,
}

#[derive(Debug)]
struct Rule {
    owners: Vec<CodeOwner>,
    negative: bool,
    section: usize,
}

/// A CODEOWNERS file as herald read it: the lines it reads, in the file's
/// order, compiled together, and the lines it skips. Made only by
/// [`Codeowners::read`].
#[derive(Debug)]
pub struct Codeowners {
    dialect: Dialect,
    rules: Vec<Rule>,
    set: Option<RegexSet>,
    sections: Vec<String>,
    skipped: Vec<Skipped>,
    refused: Option<String>,
}

/// Each line's rule and the regular expression it matches a path by, as
/// the file is read.
struct Reading {
    dialect: Dialect,
    rules: Vec<Rule>,
    patterns: Vec<String>,
    sections: Vec<String>,
    section: usize,
    defaults: Vec<CodeOwner>,
    skipped: Vec<Skipped>,
}

impl Reading {
    fn skip(&mut self, line: usize, why: String) {
        self.skipped.push(Skipped { line, why });
    }

    fn section(&mut self, name: String) -> usize {
        let lowered = name.to_lowercase();
        if let Some(at) = self
            .sections
            .iter()
            .position(|known| known.to_lowercase() == lowered)
        {
            return at;
        }
        self.sections.push(name);
        self.sections.len() - 1
    }

    fn line(&mut self, number: usize, line: &str) {
        if self.dialect == Dialect::GitLab
            && let Some(headed) = heading(line)
        {
            match headed {
                Ok(headed) => {
                    self.section = self.section(headed.name);
                    self.defaults = headed.owners;
                }
                Err(why) => self.skip(number, why),
            }
            return;
        }
        if let Err(why) = self.rule(line) {
            self.skip(number, why);
        }
    }

    fn rule(&mut self, line: &str) -> Result<(), String> {
        let words = words(line, self.dialect);
        let Some((written, named)) = words.split_first() else {
            return Err("it names no pattern".to_owned());
        };
        if written.chars().count() > MOST_CODEOWNERS_PATTERN {
            return Err(format!(
                "its pattern is longer than {MOST_CODEOWNERS_PATTERN} characters"
            ));
        }
        let mut owners = owners(named, self.dialect)?;
        let (negative, expression) = match self.dialect {
            Dialect::GitHub => (false, pattern::github(written)?),
            Dialect::GitLab => match written.strip_prefix('!') {
                Some(exempted) => (true, pattern::gitlab(exempted)),
                None => (false, pattern::gitlab(written)),
            },
            Dialect::Forgejo => match written.strip_prefix('!') {
                Some(negated) => (true, pattern::forgejo(negated)),
                None => (false, pattern::forgejo(written)),
            },
        };
        if self.dialect == Dialect::Forgejo && owners.is_empty() {
            return Err("it names no owner Forgejo reads".to_owned());
        }
        if self.dialect == Dialect::GitLab && owners.is_empty() {
            owners.clone_from(&self.defaults);
        }
        compiles(&expression)?;
        self.rules.push(Rule {
            owners,
            negative,
            section: self.section,
        });
        self.patterns.push(expression);
        Ok(())
    }
}

/// Whether `expression` compiles alone within [`MOST_RULE_COMPILED`].
fn compiles(expression: &str) -> Result<(), String> {
    RegexBuilder::new(expression)
        .size_limit(MOST_RULE_COMPILED)
        .build()
        .map(drop)
        .map_err(|error| match error {
            regex::Error::CompiledTooBig(_) => {
                format!("its pattern compiles to more than {MOST_RULE_COMPILED} bytes")
            }
            other => format!(
                "its pattern is no regular expression herald reads: {}",
                other
                    .to_string()
                    .lines()
                    .find_map(|line| line.strip_prefix("error: "))
                    .unwrap_or("it does not compile")
            ),
        })
}

impl Codeowners {
    /// A file of `dialect`'s forge herald reads none of, as `why` says.
    fn refusing(dialect: Dialect, why: String) -> Self {
        Self {
            dialect,
            rules: Vec::new(),
            set: None,
            sections: Vec::new(),
            skipped: Vec::new(),
            refused: Some(why),
        }
    }

    /// The CODEOWNERS file `body`, written for `dialect`'s forge, read
    /// only where it is at most `most` bytes.
    #[must_use]
    pub fn within(dialect: Dialect, body: &[u8], most: usize) -> Self {
        if body.len() > most {
            return Self::refusing(
                dialect,
                format!(
                    "it is longer than {most} bytes, the most the source reads; raise most_codeowners, at most {MOST_CODEOWNERS}"
                ),
            );
        }
        Self::read(dialect, &String::from_utf8_lossy(body))
    }

    /// The CODEOWNERS file `text`, written for `dialect`'s forge.
    #[must_use]
    pub fn read(dialect: Dialect, text: &str) -> Self {
        if text.len() > MOST_CODEOWNERS {
            return Self::refusing(
                dialect,
                format!("it is longer than {MOST_CODEOWNERS} bytes, GitHub's own limit"),
            );
        }
        let lines = text.lines().count();
        if lines > MOST_CODEOWNERS_LINES {
            return Self::refusing(
                dialect,
                format!("it has {lines} lines, and at most {MOST_CODEOWNERS_LINES} are read"),
            );
        }
        let mut read = Self {
            dialect,
            rules: Vec::new(),
            set: None,
            sections: Vec::new(),
            skipped: Vec::new(),
            refused: None,
        };
        let mut reading = Reading {
            dialect,
            rules: Vec::new(),
            patterns: Vec::new(),
            sections: vec![DEFAULT_SECTION.to_owned()],
            section: 0,
            defaults: Vec::new(),
            skipped: Vec::new(),
        };
        for (at, line) in text.lines().enumerate() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                reading.line(at + 1, line);
            }
        }
        read.skipped = reading.skipped;
        read.sections = reading.sections;
        if reading.patterns.is_empty() {
            return read;
        }
        match RegexSetBuilder::new(&reading.patterns)
            .size_limit(MOST_RULES_COMPILED)
            .dfa_size_limit(RULES_CACHE)
            .build()
        {
            Ok(set) => {
                read.set = Some(set);
                read.rules = reading.rules;
                let named = read.named();
                if named > MOST_AREAS {
                    return Self::refusing(
                        dialect,
                        format!("it names {named} areas, and at most {MOST_AREAS} are read"),
                    );
                }
            }
            Err(_) => {
                read.refused = Some(format!(
                    "its patterns compile to more than {MOST_RULES_COMPILED} bytes together"
                ));
            }
        }
        read
    }

    /// How many distinct areas its lines and sections can name.
    fn named(&self) -> usize {
        let owners = self
            .rules
            .iter()
            .flat_map(|rule| &rule.owners)
            .filter(|owner| !matches!(owner, CodeOwner::Email(_)))
            .map(CodeOwner::written);
        let sections = self
            .sections
            .iter()
            .skip(1)
            .map(String::as_str)
            .filter(|name| unnamed(name).is_none());
        owners.chain(sections).collect::<BTreeSet<_>>().len()
    }

    /// Each GitLab top-level `@name` its lines write, once, in the file's
    /// order: what only a lookup tells a group from a person.
    #[must_use]
    pub fn undecided(&self) -> Vec<&str> {
        let mut seen = BTreeSet::new();
        self.rules
            .iter()
            .flat_map(|rule| &rule.owners)
            .filter_map(|owner| match owner {
                CodeOwner::Either(name) => Some(name.as_str()),
                _ => None,
            })
            .filter(|name| seen.insert(*name))
            .collect()
    }

    /// The areas `owned` names, in name order: each team, group, role and
    /// person, and each section that names an area; never an email; a GitLab `@name` of
    /// the kind `resolved` tells, or of kind name where it tells none
    /// (ADR-0073).
    #[must_use]
    pub fn areas(&self, owned: &Owned, resolved: impl Fn(&str) -> Option<AreaKind>) -> Vec<Area> {
        let owners = owned.owners.iter().filter_map(|owner| {
            let kind = match owner {
                CodeOwner::Team(_) if self.dialect == Dialect::GitLab => AreaKind::Group,
                CodeOwner::Team(_) => AreaKind::Team,
                CodeOwner::Person(_) => AreaKind::Person,
                CodeOwner::Either(name) => resolved(name).unwrap_or(AreaKind::Name),
                CodeOwner::Role(_) => AreaKind::Role,
                CodeOwner::Email(_) => return None,
            };
            Some(Area {
                name: owner.written().to_owned(),
                kind,
            })
        });
        let sections = owned.sections.iter().map(|name| Area {
            name: name.clone(),
            kind: AreaKind::Section,
        });
        owners
            .chain(sections)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The lines herald skips, in the file's order.
    #[must_use]
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// Why herald reads none of the file, where it does not: then no path
    /// falls to any owner.
    #[must_use]
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// The owners and named sections any of `changed`'s paths falls to:
    /// none where its paths were not read. A path longer than
    /// [`MOST_AREA_PATH`] characters falls to none.
    #[must_use]
    pub fn owning(&self, changed: Option<&Changed>) -> Owned {
        let mut owned = Owned::default();
        let Some(set) = &self.set else {
            return owned;
        };
        for path in changed
            .into_iter()
            .flat_map(|changed| &changed.paths)
            .filter(|path| path.chars().count() <= MOST_AREA_PATH)
        {
            let matched = set.matches(path);
            match self.dialect {
                Dialect::GitHub => {
                    if let Some(rule) = matched.iter().next_back().and_then(|at| self.rules.get(at))
                    {
                        owned.owners.extend(rule.owners.iter().cloned());
                    }
                }
                Dialect::GitLab => self.sectioned(&matched, &mut owned),
                Dialect::Forgejo => {
                    for (at, rule) in self.rules.iter().enumerate() {
                        if matched.matched(at) != rule.negative {
                            owned.owners.extend(rule.owners.iter().cloned());
                        }
                    }
                }
            }
        }
        owned
    }

    /// GitLab: in each section the last entry a path matches names its
    /// owners, unless an exemption in the section matches it.
    fn sectioned(&self, matched: &SetMatches, owned: &mut Owned) {
        let mut winning: BTreeMap<usize, (bool, Option<&Rule>)> = BTreeMap::new();
        for (at, rule) in self.rules.iter().enumerate() {
            if !matched.matched(at) {
                continue;
            }
            let (exempt, last) = winning.entry(rule.section).or_default();
            if rule.negative {
                *exempt = true;
            } else {
                *last = Some(rule);
            }
        }
        for (section, (exempt, last)) in winning {
            if let (false, Some(rule)) = (exempt, last) {
                owned.owners.extend(rule.owners.iter().cloned());
                if let Some(name) = self
                    .sections
                    .get(section)
                    .filter(|name| section != 0 && unnamed(name).is_none())
                {
                    owned.sections.insert(name.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

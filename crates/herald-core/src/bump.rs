//! Reading the change a bot's pull request proposes from its title: the
//! package and the version it goes to, so the same bump in every
//! repository is one.

use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

/// The longest title a bump pattern reads, in characters.
const LONGEST_TITLE: usize = 256;

/// A bot herald ships a reader of bump titles for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bumper {
    /// Dependabot: `Bump x from 1 to 2`.
    Dependabot,
    /// Renovate: `Update dependency x to v2`, or `Update x to v2`.
    Renovate,
}

impl Bumper {
    /// Every shipped reader, in the order titles are read by them.
    pub const ALL: [Self; 2] = [Self::Dependabot, Self::Renovate];

    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dependabot => "dependabot",
            Self::Renovate => "renovate",
        }
    }

    /// The patterns it reads titles by, each with or without a
    /// conventional prefix and a trailing note.
    #[must_use]
    pub const fn patterns(self) -> &'static [&'static str] {
        match self {
            Self::Dependabot => &["*bump {package} from * to {to}*"],
            Self::Renovate => &[
                "*update dependency {package} to {to}*",
                "*update {package} to {to}*",
            ],
        }
    }
}

/// Every shipped reader's patterns, in the order [`Bumper::ALL`] lists them.
static SHIPPED: LazyLock<Vec<BumpPattern>> = LazyLock::new(|| {
    Bumper::ALL
        .iter()
        .flat_map(|bumper| bumper.patterns())
        .filter_map(|text| BumpPattern::new(text).ok())
        .collect()
});

/// One part of a bump pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    /// This character, ignoring ASCII case.
    Character(char),
    /// Any run of characters.
    Any,
    /// The package: a run of one character or more.
    Package,
    /// The version: a run of characters other than whitespace, as long as
    /// it goes.
    To,
}

/// A pattern a bump's title is read by: `{package}` stands for the
/// package, `{to}` for the version it goes to, `*` for any run of
/// characters, and every other character for itself, ignoring ASCII case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BumpPattern {
    written: String,
    parts: Vec<Part>,
}

/// Why a bump pattern is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotABumpPattern {
    /// It names no `{package}`.
    NoPackage,
    /// It names `{package}` or `{to}` more than once.
    Repeated(&'static str),
    /// It names a placeholder herald does not know.
    Unknown(String),
    /// It opens a placeholder it never closes.
    Unclosed,
}

impl fmt::Display for NotABumpPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPackage => f.write_str("a bump pattern names the package as {package}"),
            Self::Unclosed => f.write_str("a bump pattern closes each {placeholder}"),
            Self::Repeated(name) => write!(f, "a bump pattern names {{{name}}} once"),
            Self::Unknown(name) => write!(
                f,
                "a bump pattern names {{package}} and {{to}}, not {{{name}}}"
            ),
        }
    }
}

impl BumpPattern {
    /// The pattern `text` writes.
    ///
    /// # Errors
    ///
    /// Where it names no `{package}`, a placeholder twice, or one herald
    /// does not know.
    pub fn new(text: &str) -> Result<Self, NotABumpPattern> {
        let mut parts = Vec::new();
        let mut characters = text.chars();
        while let Some(character) = characters.next() {
            let part = match character {
                '*' => Part::Any,
                '{' => {
                    let (name, after) = characters
                        .as_str()
                        .split_once('}')
                        .ok_or(NotABumpPattern::Unclosed)?;
                    let name = name.to_owned();
                    characters = after.chars();
                    match name.as_str() {
                        "package" => Part::Package,
                        "to" => Part::To,
                        _ => return Err(NotABumpPattern::Unknown(name)),
                    }
                }
                other => Part::Character(other),
            };
            parts.push(part);
        }
        for (part, name) in [(Part::Package, "package"), (Part::To, "to")] {
            if parts.iter().filter(|held| **held == part).count() > 1 {
                return Err(NotABumpPattern::Repeated(name));
            }
        }
        if !parts.contains(&Part::Package) {
            return Err(NotABumpPattern::NoPackage);
        }
        Ok(Self {
            written: text.to_owned(),
            parts,
        })
    }

    /// The pattern, as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.written
    }

    /// The bump `title` names, where the whole of it matches.
    #[must_use]
    pub fn read(&self, title: &str) -> Option<Bump> {
        let text: Vec<char> = title.chars().take(LONGEST_TITLE).collect();
        let mut reading = Reading {
            parts: &self.parts,
            text: &text,
            failed: HashSet::new(),
            package: None,
            to: None,
        };
        if !reading.matches(0, 0) {
            return None;
        }
        let taken = |(start, end): (usize, usize)| text.get(start..end).map(String::from_iter);
        let package = reading.package.and_then(taken)?;
        let to = reading.to.and_then(taken).map(|to| version(&to));
        Some(Bump { package, to })
    }
}

/// `to` without a leading `v` before a digit, so `v2.0` and `2.0` are one
/// version.
fn version(to: &str) -> String {
    to.strip_prefix(['v', 'V'])
        .filter(|rest| rest.starts_with(|character: char| character.is_ascii_digit()))
        .unwrap_or(to)
        .to_owned()
}

/// A match of a pattern's parts against a title, remembering where it
/// failed so no position is tried twice.
struct Reading<'a> {
    parts: &'a [Part],
    text: &'a [char],
    failed: HashSet<(usize, usize)>,
    package: Option<(usize, usize)>,
    to: Option<(usize, usize)>,
}

impl Reading<'_> {
    /// Whether the parts from `part` match the text from `at`, keeping
    /// where the placeholders matched.
    fn matches(&mut self, part: usize, at: usize) -> bool {
        if self.failed.contains(&(part, at)) {
            return false;
        }
        if self.matched(part, at) {
            return true;
        }
        self.failed.insert((part, at));
        false
    }

    /// As [`Reading::matches`], unremembered.
    fn matched(&mut self, part: usize, at: usize) -> bool {
        let Some(wanted) = self.parts.get(part) else {
            return at == self.text.len();
        };
        match wanted {
            Part::Character(character) => {
                self.text
                    .get(at)
                    .is_some_and(|held| held.eq_ignore_ascii_case(character))
                    && self.matches(part + 1, at + 1)
            }
            Part::Any => (at..=self.text.len()).any(|end| self.matches(part + 1, end)),
            Part::Package => ((at + 1)..=self.text.len()).any(|end| {
                let matched = self.matches(part + 1, end);
                if matched {
                    self.package = Some((at, end));
                }
                matched
            }),
            Part::To => {
                let end = self
                    .text
                    .iter()
                    .skip(at)
                    .position(|character| character.is_whitespace())
                    .map_or(self.text.len(), |run| at + run);
                let matched = end > at && self.matches(part + 1, end);
                if matched {
                    self.to = Some((at, end));
                }
                matched
            }
        }
    }
}

/// The same change a bot proposes in each repository: a package, and the
/// version it goes to where the title names one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bump {
    /// The package, as the title names it.
    pub package: String,
    /// The version it goes to.
    pub to: Option<String>,
}

/// The bump `title` names by the first of `patterns` that reads it.
#[must_use]
pub fn bumped(patterns: &[BumpPattern], title: &str) -> Option<Bump> {
    patterns.iter().find_map(|pattern| pattern.read(title))
}

/// The bump `title` names by the first of `own` that reads it, then by
/// herald's shipped readers.
#[must_use]
pub fn read_bump(own: &[BumpPattern], title: &str) -> Option<Bump> {
    bumped(own, title).or_else(|| bumped(&SHIPPED, title))
}

#[cfg(test)]
mod tests;

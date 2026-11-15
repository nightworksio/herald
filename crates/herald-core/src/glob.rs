use std::fmt;

/// A pattern a name is matched against, ignoring case: `*` stands for any
/// run of characters, `?` for any one, `[...]` for any one it lists or
/// ranges over (`[!...]` or `[^...]` for any one it does not), and every
/// other character for itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    written: String,
    parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Any,
    One,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
    Literal(char),
}

impl Part {
    /// Whether one character could match both it and `other`.
    fn shares(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Literal(character), part) | (part, Self::Literal(character)) => {
                part.takes(*character)
            }
            (
                Self::Class {
                    negated: false,
                    ranges: ours,
                },
                Self::Class {
                    negated: false,
                    ranges: theirs,
                },
            ) => ours
                .iter()
                .any(|&(low, high)| theirs.iter().any(|&(from, to)| low <= to && from <= high)),
            _ => true,
        }
    }

    fn takes(&self, character: char) -> bool {
        match self {
            Self::Any | Self::One => true,
            Self::Class { negated, ranges } => {
                ranges
                    .iter()
                    .any(|&(low, high)| (low..=high).contains(&character))
                    != *negated
            }
            Self::Literal(wanted) => *wanted == character,
        }
    }
}

/// Why a glob could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAGlob(String);

impl fmt::Display for NotAGlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Glob {
    /// The glob `text` writes.
    ///
    /// # Errors
    ///
    /// Where a `[` is never closed, or closes with nothing in it.
    pub fn new(text: &str) -> Result<Self, NotAGlob> {
        let lowered: Vec<char> = text.to_lowercase().chars().collect();
        let mut parts = Vec::new();
        let mut at = 0;
        while let Some(&character) = lowered.get(at) {
            at += 1;
            parts.push(match character {
                '*' => Part::Any,
                '?' => Part::One,
                '[' => {
                    let (class, after) = class(&lowered, at).ok_or_else(|| {
                        NotAGlob(format!(
                            "\"{text}\" opens a [ it never closes with a character in it; close it, or write [[] for a [ itself"
                        ))
                    })?;
                    at = after;
                    class
                }
                literal => Part::Literal(literal),
            });
        }
        Ok(Self {
            written: text.to_owned(),
            parts,
        })
    }

    /// How narrowly it names: whether it holds no wildcard, then how many
    /// characters it names as themselves. A narrower glob orders after a
    /// wider one.
    #[must_use]
    pub fn specificity(&self) -> (bool, usize) {
        let literal = self
            .parts
            .iter()
            .filter(|part| matches!(part, Part::Literal(_)))
            .count();
        (literal == self.parts.len(), literal)
    }

    /// The glob, as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.written
    }

    /// Whether the whole of `name` matches, ignoring case.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        let name: Vec<char> = name.to_lowercase().chars().collect();
        wildcards(&self.parts, |part| *part == Part::Any, Part::takes, &name)
    }

    /// Whether some name could match both it and `other`, ignoring case. A
    /// negated class is taken to share a character with any other class.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        let (ours, theirs) = (&self.parts, &other.parts);
        let width = theirs.len() + 1;
        let mut reached = vec![false; (ours.len() + 1) * width];
        for at in 0..=ours.len() {
            for to in 0..=theirs.len() {
                let was = |at: usize, to: usize| reached.get(at * width + to) == Some(&true);
                let our = at.checked_sub(1).and_then(|before| ours.get(before));
                let their = to.checked_sub(1).and_then(|before| theirs.get(before));
                let here = match (our, their) {
                    (None, None) => true,
                    (Some(our), None) => *our == Part::Any && was(at - 1, to),
                    (None, Some(their)) => *their == Part::Any && was(at, to - 1),
                    (Some(our), Some(their)) => {
                        (*our == Part::Any && (was(at - 1, to) || was(at, to - 1)))
                            || (*their == Part::Any && (was(at, to - 1) || was(at - 1, to)))
                            || (was(at - 1, to - 1) && our.shares(their))
                    }
                };
                if let Some(cell) = reached.get_mut(at * width + to) {
                    *cell = here;
                }
            }
        }
        reached.last() == Some(&true)
    }
}

/// Whether the whole of `text` matches `parts` in turn, each a run of any
/// characters where `any` says so and otherwise one character it `takes`.
pub(crate) fn wildcards<P>(
    parts: &[P],
    any: impl Fn(&P) -> bool,
    takes: impl Fn(&P, char) -> bool,
    text: &[char],
) -> bool {
    let mut matched = vec![true];
    matched.resize(text.len() + 1, false);
    for part in parts {
        matched = if any(part) {
            let mut reached = false;
            matched
                .iter()
                .map(|&before| {
                    reached |= before;
                    reached
                })
                .collect()
        } else {
            std::iter::once(false)
                .chain(
                    matched
                        .iter()
                        .zip(text)
                        .map(|(&before, &character)| before && takes(part, character)),
                )
                .collect()
        };
    }
    matched.last().copied().unwrap_or_default()
}

/// The class that opens before `at` in `glob`, and where the glob goes on
/// after it; none where it is never closed or holds nothing.
fn class(glob: &[char], at: usize) -> Option<(Part, usize)> {
    let (negated, mut rest) = match glob.get(at..)? {
        ['!' | '^', after @ ..] => (true, after),
        rest => (false, rest),
    };
    let mut ranges = Vec::new();
    loop {
        rest = match rest {
            [']', after @ ..] if !ranges.is_empty() => {
                return Some((Part::Class { negated, ranges }, glob.len() - after.len()));
            }
            [low, '-', high, after @ ..] if *high != ']' => {
                ranges.push((*low.min(high), *low.max(high)));
                after
            }
            [low, after @ ..] => {
                ranges.push((*low, *low));
                after
            }
            [] => return None,
        };
    }
}

impl fmt::Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(text: &str) -> Glob {
        Glob::new(text).unwrap()
    }

    #[test]
    fn two_globs_overlap_where_some_name_matches_both() {
        let overlap = |one: &str, other: &str| {
            let (one, other) = (Glob::new(one).unwrap(), Glob::new(other).unwrap());
            assert_eq!(one.overlaps(&other), other.overlaps(&one));
            one.overlaps(&other)
        };
        assert!(overlap("plugin-*", "*-sonarr"));
        assert!(overlap("a*", "abc"));
        assert!(overlap("abc", "a*"));
        assert!(overlap("a*", "a"));
        assert!(overlap("a", "a*"));
        assert!(overlap("PLUGIN-a", "plugin-?"));
        assert!(overlap("*", ""));
        assert!(overlap("a*b", "a*b*"));
        assert!(overlap("[a-c]x", "[b-d]?"));
        assert!(overlap("[!a]x", "[b]x"));
        assert!(overlap("*a*", "*b*"));
        assert!(!overlap("plugin-*", "web-*"));
        assert!(!overlap("a?", "a"));
        assert!(!overlap("[a-b]x", "[c-d]x"));
        assert!(!overlap("a", ""));
        assert!(!overlap("x[a]", "x[b]"));
    }

    #[test]
    fn a_glob_without_wildcards_is_narrowest_and_more_named_characters_narrower() {
        let narrow = |text: &str| Glob::new(text).unwrap().specificity();
        assert_eq!(narrow("plugin-sonarr"), (true, 13));
        assert_eq!(narrow("plugin-*"), (false, 7));
        assert_eq!(narrow("plugin-[ab]?"), (false, 7));
        assert_eq!(narrow("*"), (false, 0));
        assert_eq!(narrow(""), (true, 0));
        assert!(narrow("*") < narrow("p*") && narrow("plugin-*") < narrow("plugin-a"));
    }

    #[test]
    fn a_glob_matches_whole_names_ignoring_case() {
        let plugins = glob("Plugin-*");
        assert!(plugins.matches("plugin-sonarr"));
        assert!(plugins.matches("PLUGIN-"));
        assert!(!plugins.matches("lemonfiber-plugin-x"));
        assert!(glob("lemon?iber").matches("LemonFiber"));
        assert!(!glob("lemon?iber").matches("lemonfiber-x"));
        assert!(glob("*").matches(""));
        assert!(glob("").matches(""));
        assert!(!glob("").matches("a"));
        assert!(glob("a*b*c").matches("aXXbYc"));
        assert!(!glob("a*b*c").matches("aXXcYb"));
        assert_eq!(plugins.as_str(), "Plugin-*");
        assert_eq!(plugins.to_string(), "Plugin-*");
    }

    #[test]
    fn a_class_matches_one_character_it_lists_or_ranges_over() {
        let tiers = glob("tier-[a-c0]");
        assert!(tiers.matches("tier-b"));
        assert!(tiers.matches("TIER-B"));
        assert!(tiers.matches("tier-0"));
        assert!(!tiers.matches("tier-d"));
        assert!(!tiers.matches("tier-bb"));
        assert!(glob("[c-a]").matches("b"));
        for negated in ["x[!0-9]", "x[^0-9]"] {
            assert!(glob(negated).matches("xa"), "{negated}");
            assert!(!glob(negated).matches("x5"), "{negated}");
        }
        assert!(glob("[]]").matches("]"));
        assert!(glob("[[]").matches("["));
        assert!(glob("[a-]").matches("-"));
        assert!(glob("[a-]").matches("a"));
        assert!(!glob("[a-]").matches("b"));
        assert!(glob("[!a]").matches("!"));
        assert!(glob("[^a]").matches("^"));
        assert!(glob("[ab]c?").matches("acx"));
        assert!(!glob("[ab]c?").matches("ac"));
    }

    #[test]
    fn a_class_never_closed_is_refused() {
        for unclosed in ["plugin-[a-z", "[", "[!", "[]"] {
            assert_eq!(
                Glob::new(unclosed).map_err(|refused| refused.to_string()),
                Err(format!(
                    "\"{unclosed}\" opens a [ it never closes with a character in it; close it, or write [[] for a [ itself"
                )),
                "{unclosed}"
            );
        }
    }
}

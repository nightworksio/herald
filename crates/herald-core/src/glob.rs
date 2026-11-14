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
fn class(glob: &[char], mut at: usize) -> Option<(Part, usize)> {
    let negated = matches!(glob.get(at), Some('!' | '^'));
    if negated {
        at += 1;
    }
    let mut ranges = Vec::new();
    loop {
        let low = *glob.get(at)?;
        if low == ']' && !ranges.is_empty() {
            return Some((Part::Class { negated, ranges }, at + 1));
        }
        match (glob.get(at + 1), glob.get(at + 2)) {
            (Some('-'), Some(&high)) if high != ']' => {
                ranges.push((low.min(high), low.max(high)));
                at += 3;
            }
            _ => {
                ranges.push((low, low));
                at += 1;
            }
        }
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

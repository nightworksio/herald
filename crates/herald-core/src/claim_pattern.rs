use std::fmt;

use crate::pattern::Pattern;

/// What stands for other characters in a pattern.
const WILDCARDS: [char; 2] = ['*', '?'];

/// A pattern a claim of an ID token is matched against, case and all. The
/// pattern and the claim are cut at each `/` into as many parts as each
/// other, and each part of the claim matches its part of the pattern, where
/// `*` stands for any run of characters and `?` for any one, neither of
/// which reaches past a `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimPattern(String);

impl ClaimPattern {
    /// The pattern `text` writes.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The pattern, as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it matches one claim alone: whether it writes no wildcard
    /// and is not empty.
    #[must_use]
    pub fn literal(&self) -> bool {
        !self.0.is_empty() && !self.0.contains(WILDCARDS)
    }

    /// Whether it fixes what comes before a claim's first `/`, its owner
    /// in a path: whether that part writes no wildcard and is not empty.
    #[must_use]
    pub fn owned(&self) -> bool {
        self.0
            .split('/')
            .next()
            .is_some_and(|owner| !owner.is_empty() && !owner.contains(WILDCARDS))
    }

    /// Whether the whole of `claim` matches it.
    #[must_use]
    pub fn matches(&self, claim: &str) -> bool {
        let wanted: Vec<&str> = self.0.split('/').collect();
        let parts: Vec<&str> = claim.split('/').collect();
        wanted.len() == parts.len()
            && wanted
                .iter()
                .zip(&parts)
                .all(|(wanted, part)| Pattern::new(*wanted).matches(part))
    }
}

impl fmt::Display for ClaimPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_stays_within_its_part_and_case_counts() {
        let owned = ClaimPattern::new("org/*");
        for (claim, matched) in [
            ("org/x", true),
            ("org/plugin-sonarr", true),
            ("org/", true),
            ("org/a/b", false),
            ("org-evil/x", false),
            ("Org/x", false),
            ("org", false),
            ("xorg/x", false),
            ("org/x/", false),
        ] {
            assert_eq!(owned.matches(claim), matched, "{claim}");
        }
        assert!(ClaimPattern::new("refs/heads/main").matches("refs/heads/main"));
        assert!(!ClaimPattern::new("refs/heads/main").matches("refs/heads/main2"));
        assert!(ClaimPattern::new("refs/tags/v?.*").matches("refs/tags/v1.2"));
        assert!(!ClaimPattern::new("refs/tags/v?.*").matches("refs/tags/v12.2"));
        assert!(ClaimPattern::new("*").matches("1234"));
        assert!(!ClaimPattern::new("*").matches("a/b"));
        assert_eq!(ClaimPattern::new("o/*").to_string(), "o/*");
        assert_eq!(ClaimPattern::new("o/*").as_str(), "o/*");
    }

    #[test]
    fn a_pattern_is_literal_without_wildcards_and_owned_where_its_first_part_is() {
        for (pattern, literal, owned) in [
            ("123456", true, true),
            ("org/plugin", true, true),
            ("org/*", false, true),
            ("repo:org/*:ref:refs/heads/main", false, true),
            ("repo:org*", false, false),
            ("org?/x", false, false),
            ("*/plugin", false, false),
            ("/plugin", true, false),
            ("*", false, false),
            ("?", false, false),
            ("", false, false),
        ] {
            let read = ClaimPattern::new(pattern);
            assert_eq!(
                (read.literal(), read.owned()),
                (literal, owned),
                "{pattern}"
            );
        }
    }
}

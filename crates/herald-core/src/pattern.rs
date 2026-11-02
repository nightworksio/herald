use std::fmt;

/// A pattern a route matches versions against: `*` stands for any run of
/// characters, `?` for any one character, and every other character for
/// itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern(String);

impl Pattern {
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

    /// Whether `text` matches the whole pattern.
    #[must_use]
    pub fn matches(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        let mut matched = vec![true];
        matched.resize(text.len() + 1, false);
        for wanted in self.0.chars() {
            matched = if wanted == '*' {
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
                    .chain(matched.iter().zip(&text).map(|(&before, &character)| {
                        before && (wanted == '?' || character == wanted)
                    }))
                    .collect()
            };
        }
        matched.last().copied().unwrap_or_default()
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn a_star_stands_for_any_run_and_a_question_mark_for_one_character() {
        let release = Pattern::new("*.*.*");
        for (version, matched) in [
            ("2.11.7", true),
            ("v1.0.0-rc.1", true),
            ("latest", false),
            ("2.11", false),
            ("..", true),
        ] {
            assert_eq!(release.matches(version), matched, "{version}");
        }
        let one = Pattern::new("v?.?");
        assert!(one.matches("v1.2"));
        assert!(!one.matches("v10.2"));
        assert!(!one.matches("v1."));
        assert!(Pattern::new("").matches(""));
        assert!(!Pattern::new("").matches("1"));
        assert!(Pattern::new("**").matches(""));
        assert!(Pattern::new("*a*").matches("aa"));
        assert!(Pattern::new("*-alpine").matches("2.11.7-builder-alpine"));
        assert!(!Pattern::new("*-alpine").matches("2.11.7-alpine3"));
        assert_eq!(Pattern::new("*.*").to_string(), "*.*");
        assert_eq!(Pattern::new("*.*").as_str(), "*.*");
    }

    proptest! {
        #[test]
        fn a_pattern_without_wildcards_matches_only_itself(text in "[a-z0-9.-]{0,12}", other in "[a-z0-9.-]{0,12}") {
            prop_assert!(Pattern::new(text.clone()).matches(&text));
            prop_assert_eq!(Pattern::new(text.clone()).matches(&other), text == other);
        }

        #[test]
        fn a_star_around_any_part_finds_it_anywhere(before in "[a-z0-9.]{0,8}", part in "[a-z0-9.]{0,6}", after in "[a-z0-9.]{0,8}") {
            let whole = format!("{before}{part}{after}");
            let around = Pattern::new(format!("*{part}*"));
            prop_assert!(around.matches(&whole));
            prop_assert!(Pattern::new("*").matches(&whole));
            let marks = "?".repeat(whole.chars().count());
            prop_assert!(Pattern::new(marks.clone()).matches(&whole));
            let longer = Pattern::new(format!("{marks}?"));
            prop_assert!(!longer.matches(&whole));
        }
    }
}

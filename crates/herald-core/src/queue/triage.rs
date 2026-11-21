//! Check triage: the checks failing across a view's open pull requests,
//! whether so many fail one that its base is broken, and which are flaky
//! candidates (ADR-0065).

use std::collections::BTreeMap;

use jiff::SignedDuration;

use super::{Checks, Row};

/// How a view triages its failing checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triage {
    /// How many open pull requests one check fails on before the base
    /// they merge into reads as broken.
    pub broken_at: usize,
    /// How far back a check failing then passing on one head counts.
    pub flaky_window: SignedDuration,
    /// How many such flips within the window make a check a flaky
    /// candidate.
    pub flaky_least: usize,
}

impl Default for Triage {
    fn default() -> Self {
        Self {
            broken_at: 3,
            flaky_window: SignedDuration::from_hours(7 * 24),
            flaky_least: 1,
        }
    }
}

/// One check failing on a view's open pull requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The check's name.
    pub check: String,
    /// The pull requests it fails on, by repository and number.
    pub pulls: Vec<(String, u64)>,
    /// Whether it fails on so many that their base reads as broken.
    pub broken: bool,
    /// How many times it failed then passed on one head within the window,
    /// across the view's open pull requests.
    pub flips: usize,
    /// Whether that makes it a flaky candidate.
    pub flaky: bool,
}

impl Triage {
    /// Each check failing on `rows`, those failing on the most first, then
    /// by name.
    #[must_use]
    pub fn failures(&self, rows: &[Row]) -> Vec<Failure> {
        let mut failing: BTreeMap<&str, Vec<(String, u64)>> = BTreeMap::new();
        let mut flips: BTreeMap<&str, usize> = BTreeMap::new();
        for row in rows {
            if let Checks::Failed(names) = &row.pull.checks {
                for name in names {
                    failing
                        .entry(name)
                        .or_default()
                        .push((row.pull.repository.clone(), row.pull.number));
                }
            }
            for flip in &row.pull.flips {
                *flips.entry(&flip.check).or_default() += 1;
            }
        }
        let mut failures: Vec<Failure> = failing
            .into_iter()
            .map(|(check, pulls)| {
                let flipped = flips.get(check).copied().unwrap_or_default();
                Failure {
                    check: check.to_owned(),
                    broken: pulls.len() >= self.broken_at,
                    pulls,
                    flips: flipped,
                    flaky: flipped >= self.flaky_least,
                }
            })
            .collect();
        failures.sort_by_key(|failure| std::cmp::Reverse(failure.pulls.len()));
        failures
    }
}

#[cfg(test)]
mod tests;

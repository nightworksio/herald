//! Which open pull requests would change the same files: those of one
//! repository merging into one branch whose heads change a path in common
//! (ADR-0063).

use std::collections::BTreeMap;

use super::{Changed, Pull};

/// The pull requests of `pulls` each shares a changed path with, by its
/// repository and number: those of its repository merging into its branch,
/// by number, the lowest first. One whose branch or changed paths were not
/// read shares none.
#[must_use]
pub fn overlaps(pulls: &[Pull]) -> BTreeMap<(String, u64), Vec<u64>> {
    let mut sharing: BTreeMap<(String, u64), Vec<u64>> = BTreeMap::new();
    let mut bases: BTreeMap<(&str, &str), Vec<(&Pull, &Changed)>> = BTreeMap::new();
    for pull in pulls {
        if let (Some(base), Some(files)) = (pull.base.as_deref(), &pull.files) {
            bases
                .entry((pull.repository.as_str(), base))
                .or_default()
                .push((pull, files));
        }
    }
    for together in bases.values() {
        for (place, (one, mine)) in together.iter().enumerate() {
            for (other, theirs) in together.iter().skip(place + 1) {
                if mine.paths.is_disjoint(&theirs.paths) {
                    continue;
                }
                for (from, to) in [(one, other), (other, one)] {
                    sharing
                        .entry((from.repository.clone(), from.number))
                        .or_default()
                        .push(to.number);
                }
            }
        }
    }
    for numbers in sharing.values_mut() {
        numbers.sort_unstable();
    }
    sharing
}

#[cfg(test)]
mod tests;

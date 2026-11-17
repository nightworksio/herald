//! The key set an instance last published, kept so its sequence rises by
//! one with each new set (ADR-0044).

use serde::{Deserialize, Serialize};

/// The keys an instance last published, newest first, each as its
/// Multikey, under the sequence it published them with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedKeys {
    /// Raised by one with each new set.
    pub sequence: u64,
    /// The keys, newest first, each as its `publicKeyMultibase`.
    pub keys: Vec<String>,
}

impl PublishedKeys {
    /// The record publishing `keys` after `held`: `held` itself where it
    /// lists the same keys in the same order, and otherwise `keys` under the
    /// next sequence, the first being 1.
    #[must_use]
    pub fn after(held: Option<&Self>, keys: Vec<String>) -> Self {
        match held {
            Some(held) if held.keys == keys => held.clone(),
            held => Self {
                sequence: held.map_or(0, |held| held.sequence).saturating_add(1),
                keys,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(listed: &[&str]) -> Vec<String> {
        listed.iter().map(|key| (*key).to_owned()).collect()
    }

    #[test]
    fn the_first_set_is_published_as_one() {
        assert_eq!(
            PublishedKeys::after(None, keys(&["z6Mka"])),
            PublishedKeys {
                sequence: 1,
                keys: keys(&["z6Mka"])
            }
        );
    }

    #[test]
    fn the_same_keys_keep_their_sequence_and_any_change_raises_it_by_one() {
        let held = PublishedKeys {
            sequence: 4,
            keys: keys(&["z6Mka"]),
        };
        assert_eq!(PublishedKeys::after(Some(&held), keys(&["z6Mka"])), held);
        for changed in [keys(&["z6Mkb", "z6Mka"]), keys(&[]), keys(&["z6Mkb"])] {
            assert_eq!(
                PublishedKeys::after(Some(&held), changed.clone()),
                PublishedKeys {
                    sequence: 5,
                    keys: changed
                }
            );
        }
        let reordered = PublishedKeys {
            sequence: 2,
            keys: keys(&["z6Mka", "z6Mkb"]),
        };
        assert_eq!(
            PublishedKeys::after(Some(&reordered), keys(&["z6Mkb", "z6Mka"])).sequence,
            3
        );
    }
}

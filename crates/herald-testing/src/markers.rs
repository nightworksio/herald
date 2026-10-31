use std::collections::BTreeMap;

use herald_core::{Entry, Key};
use herald_ports::{Markers, MarkersError};

/// A destination's posts, as the entries their markers make.
#[derive(Debug, Default)]
pub struct FakeMarkers {
    entries: BTreeMap<Key, Entry>,
    unreadable: bool,
}

impl FakeMarkers {
    /// The posts with one more, carrying `entry`'s key.
    #[must_use]
    pub fn with(mut self, entry: Entry) -> Self {
        self.entries.insert(entry.key, entry);
        self
    }

    /// Posts that cannot be read.
    #[must_use]
    pub fn unreadable() -> Self {
        Self {
            entries: BTreeMap::new(),
            unreadable: true,
        }
    }
}

impl Markers for FakeMarkers {
    async fn entry(&self, key: &Key) -> Result<Option<Entry>, MarkersError> {
        if self.unreadable {
            return Err(MarkersError::Unreadable {
                reason: "the fake destination is unreadable".to_owned(),
            });
        }
        Ok(self.entries.get(key).cloned())
    }
}

#[cfg(test)]
mod tests {
    use herald_core::{Identity, RouteId};

    use super::*;

    #[test]
    fn a_marked_post_answers_its_entry_and_an_unreadable_destination_says_so() {
        let key = Key::derive(&RouteId::new("r"), &Identity::new("i"));
        let markers = FakeMarkers::default().with(Entry::new(key));
        assert_eq!(
            pollster::block_on(markers.entry(&key)).unwrap(),
            Some(Entry::new(key))
        );
        let other = Key::derive(&RouteId::new("r"), &Identity::new("j"));
        assert_eq!(pollster::block_on(markers.entry(&other)).unwrap(), None);
        assert!(pollster::block_on(FakeMarkers::unreadable().entry(&key)).is_err());
    }
}

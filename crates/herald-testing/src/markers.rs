use std::collections::BTreeMap;

use herald_core::{Entry, Key, Marker};
use herald_ports::{Markers, MarkersError};

/// A destination's posts, as the entries their markers make.
#[derive(Debug, Default)]
pub struct FakeMarkers {
    entries: BTreeMap<Key, (Marker, Entry)>,
    unreadable: bool,
}

impl FakeMarkers {
    /// The posts with one more by herald, carrying `marker` for `entry`'s key.
    #[must_use]
    pub fn with(mut self, entry: Entry, marker: Marker) -> Self {
        self.entries.insert(entry.key, (marker, entry));
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
    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        if self.unreadable {
            return Err(MarkersError::Unreadable {
                reason: "the fake destination is unreadable".to_owned(),
            });
        }
        Ok(self
            .entries
            .get(key)
            .filter(|(carried, _)| carried == marker)
            .map(|(_, entry)| entry.clone()))
    }
}

#[cfg(test)]
mod tests {
    use herald_core::{Identity, RouteId};

    use super::*;

    #[test]
    fn a_marked_post_answers_its_entry_and_an_unreadable_destination_says_so() {
        let key = Key::derive(&RouteId::new("r"), &Identity::new("i"));
        let marker = Marker::of(b"mac");
        let markers = FakeMarkers::default().with(Entry::new(key), marker.clone());
        assert_eq!(
            pollster::block_on(markers.entry(&key, &marker)).unwrap(),
            Some(Entry::new(key))
        );
        let other = Key::derive(&RouteId::new("r"), &Identity::new("j"));
        assert_eq!(
            pollster::block_on(markers.entry(&other, &marker)).unwrap(),
            None
        );
        assert!(pollster::block_on(FakeMarkers::unreadable().entry(&key, &marker)).is_err());
    }

    #[test]
    fn a_post_carrying_another_marker_for_the_key_is_not_read_as_herald_s() {
        let key = Key::derive(&RouteId::new("r"), &Identity::new("i"));
        let markers = FakeMarkers::default().with(Entry::new(key), Marker::of(b"forged"));
        assert_eq!(
            pollster::block_on(markers.entry(&key, &Marker::of(b"mac"))).unwrap(),
            None
        );
    }
}

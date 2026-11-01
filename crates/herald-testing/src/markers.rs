use std::collections::BTreeMap;

use herald_core::{Entry, Key, Marker};
use herald_ports::{MarkedThread, Markers, MarkersError};

/// A destination's posts, as the entries their markers make.
#[derive(Debug, Default)]
pub struct FakeMarkers {
    entries: BTreeMap<Key, (Marker, Entry)>,
    threads: Vec<(Marker, MarkedThread)>,
    unreadable: bool,
}

impl FakeMarkers {
    /// The posts with one more by herald, carrying `marker` for `entry`'s key.
    #[must_use]
    pub fn with(mut self, entry: Entry, marker: Marker) -> Self {
        self.entries.insert(entry.key, (marker, entry));
        self
    }

    /// The posts with one more by herald, carrying the route marker `route`,
    /// from which `thread` was opened.
    #[must_use]
    pub fn with_thread(mut self, route: Marker, thread: MarkedThread) -> Self {
        self.threads.push((route, thread));
        self
    }

    /// Posts that cannot be read.
    #[must_use]
    pub fn unreadable() -> Self {
        Self {
            unreadable: true,
            ..Self::default()
        }
    }

    fn readable(&self) -> Result<(), MarkersError> {
        if self.unreadable {
            return Err(MarkersError::Unreadable {
                reason: "the fake destination is unreadable".to_owned(),
            });
        }
        Ok(())
    }
}

impl Markers for FakeMarkers {
    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        self.readable()?;
        Ok(self
            .entries
            .get(key)
            .filter(|(carried, _)| carried == marker)
            .map(|(_, entry)| entry.clone()))
    }

    async fn threads(&self, route: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        self.readable()?;
        Ok(self
            .threads
            .iter()
            .filter(|(carried, _)| carried == route)
            .map(|(_, thread)| thread.clone())
            .collect())
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
    fn the_threads_of_posts_carrying_a_route_marker_are_listed() {
        let thread = MarkedThread {
            thread: "7".to_owned(),
            opened: jiff::Timestamp::UNIX_EPOCH,
            archived: false,
            locked: false,
        };
        let markers = FakeMarkers::default().with_thread(Marker::of(b"route"), thread.clone());
        assert_eq!(
            pollster::block_on(markers.threads(&Marker::of(b"route"))).unwrap(),
            [thread]
        );
        assert_eq!(
            pollster::block_on(markers.threads(&Marker::of(b"other"))).unwrap(),
            []
        );
        assert!(
            pollster::block_on(FakeMarkers::unreadable().threads(&Marker::of(b"route"))).is_err()
        );
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

use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::key::Identity;

const REMEMBERED: usize = 500;

/// What a polled source has seen, and the validators its server last returned.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cursor {
    /// The identities seen, the newest first.
    pub seen: Vec<Identity>,
    /// The `ETag` the server last returned.
    pub etag: Option<String>,
    /// The `Last-Modified` the server last returned.
    pub last_modified: Option<String>,
}

/// What one poll of a source found: its events, the newest first, and the
/// validators the server returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Polled {
    /// The events, the newest first.
    pub events: Vec<Event>,
    /// The `ETag` the server returned.
    pub etag: Option<String>,
    /// The `Last-Modified` the server returned.
    pub last_modified: Option<String>,
}

impl Cursor {
    /// The cursor after a poll that found `found`, the newest first, with the
    /// validators the server returned. It remembers the newest identities
    /// only, enough to cover any one poll.
    #[must_use]
    pub fn advanced(
        &self,
        found: &[Event],
        etag: Option<String>,
        last_modified: Option<String>,
    ) -> Self {
        let mut seen: Vec<Identity> = found.iter().map(|event| event.identity().clone()).collect();
        seen.extend(
            self.seen
                .iter()
                .filter(|identity| !seen_in(found, identity))
                .cloned(),
        );
        seen.truncate(REMEMBERED);
        Self {
            seen,
            etag,
            last_modified,
        }
    }
}

fn seen_in(found: &[Event], identity: &Identity) -> bool {
    found.iter().any(|event| event.identity() == identity)
}

/// The events of a poll to announce: those `cursor` has not seen. With no
/// cursor, the source is new, and only its newest `backfill` are announced.
#[must_use]
pub fn fresh(cursor: Option<&Cursor>, found: Vec<Event>, backfill: usize) -> Vec<Event> {
    match cursor {
        Some(cursor) => found
            .into_iter()
            .filter(|event| !cursor.seen.contains(event.identity()))
            .collect(),
        None => found.into_iter().take(backfill).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Draft;
    use crate::event::tests::draft;

    fn event(identity: &str) -> Event {
        Event::normalise(Draft {
            identity: identity.to_owned(),
            ..draft()
        })
        .unwrap()
    }

    fn identities(events: &[Event]) -> Vec<&str> {
        events
            .iter()
            .map(|event| event.identity().as_str())
            .collect()
    }

    #[test]
    fn a_new_source_announces_only_its_backfill() {
        let found = vec![event("c"), event("b"), event("a")];
        assert_eq!(
            identities(&fresh(None, found.clone(), 0)),
            Vec::<&str>::new()
        );
        assert_eq!(identities(&fresh(None, found, 2)), ["c", "b"]);
    }

    #[test]
    fn a_known_source_announces_what_its_cursor_has_not_seen() {
        let cursor = Cursor {
            seen: vec![Identity::new("b"), Identity::new("a")],
            ..Cursor::default()
        };
        let found = vec![event("c"), event("b"), event("a")];
        assert_eq!(identities(&fresh(Some(&cursor), found, 0)), ["c"]);
    }

    #[test]
    fn an_advanced_cursor_keeps_the_newest_first_once_each_with_the_new_validators() {
        let cursor = Cursor {
            seen: vec![Identity::new("b"), Identity::new("a")],
            etag: Some("old".to_owned()),
            last_modified: None,
        };
        let advanced = cursor.advanced(
            &[event("c"), event("b")],
            Some("new".to_owned()),
            Some("then".to_owned()),
        );
        assert_eq!(
            advanced,
            Cursor {
                seen: vec![Identity::new("c"), Identity::new("b"), Identity::new("a")],
                etag: Some("new".to_owned()),
                last_modified: Some("then".to_owned()),
            }
        );
    }

    #[test]
    fn a_cursor_remembers_a_bounded_number_of_identities() {
        let found: Vec<Event> = (0..REMEMBERED + 10)
            .map(|index| event(&index.to_string()))
            .collect();
        let advanced = Cursor::default().advanced(&found, None, None);
        assert_eq!(advanced.seen.len(), REMEMBERED);
        assert_eq!(advanced.seen.first().map(Identity::as_str), Some("0"));
    }

    #[test]
    fn a_cursor_round_trips_through_json() {
        let cursor = Cursor {
            seen: vec![Identity::new("a")],
            etag: Some("e".to_owned()),
            last_modified: None,
        };
        let json = serde_json::to_string(&cursor).unwrap();
        assert_eq!(json, r#"{"seen":["a"],"etag":"e","last_modified":null}"#);
        assert_eq!(serde_json::from_str::<Cursor>(&json).unwrap(), cursor);
    }
}

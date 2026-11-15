use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::key::Identity;
use crate::signal::{Signal, Validators};

/// What a polled source has seen, and the validators its server last returned.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cursor {
    /// The identities seen, the newest first.
    pub seen: Vec<Identity>,
    /// The `ETag` the server last returned.
    pub etag: Option<String>,
    /// The `Last-Modified` the server last returned.
    pub last_modified: Option<String>,
    /// The project its newest event was announced as.
    #[serde(default)]
    pub project: Option<String>,
    /// The validators the server last returned for each signal asked apart
    /// from the releases.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub signals: BTreeMap<Signal, Validators>,
    /// The identities of `seen` it has seen as a tag and not yet as a
    /// Release, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tagged: Vec<Identity>,
}

/// What one poll of a source found: its events, the newest first, and the
/// validators the server returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Polled {
    /// The events, the newest first.
    pub events: Vec<Event>,
    /// The `ETag` the server returned.
    pub etag: Option<String>,
    /// The `Last-Modified` the server returned.
    pub last_modified: Option<String>,
    /// The validators the server returned for each signal asked apart from
    /// the releases.
    pub signals: BTreeMap<Signal, Validators>,
    /// The identities it found without reading them in full, newest first,
    /// which announce nothing and are remembered as seen.
    pub passed: Vec<Identity>,
}

impl Cursor {
    /// The cursor after `polled`, a poll's answer, with the validators the
    /// server returned. It remembers the newest `remembered` identities
    /// only, enough to cover any one poll.
    #[must_use]
    pub fn advanced(&self, polled: &Polled, remembered: usize) -> Self {
        let found = &polled.events;
        let mut seen: Vec<Identity> = found.iter().map(|event| event.identity().clone()).collect();
        seen.extend(
            polled
                .passed
                .iter()
                .filter(|identity| !seen_in(found, identity))
                .cloned(),
        );
        let newest = seen.clone();
        seen.extend(
            self.seen
                .iter()
                .filter(|identity| !newest.contains(identity))
                .cloned(),
        );
        seen.truncate(remembered);
        let released = |identity: &Identity| {
            found.iter().any(|event| {
                event.identity() == identity && event.signal() == Some(Signal::Release)
            })
        };
        let mut tagged: Vec<Identity> = Vec::new();
        let fresh_tags = found
            .iter()
            .filter(|event| event.signal() == Some(Signal::Tag))
            .map(Event::identity)
            .chain(&polled.passed);
        for identity in fresh_tags.chain(&self.tagged) {
            if !released(identity) && seen.contains(identity) && !tagged.contains(identity) {
                tagged.push(identity.clone());
            }
        }
        Self {
            seen,
            tagged,
            etag: polled.etag.clone(),
            last_modified: polled.last_modified.clone(),
            project: found
                .first()
                .map(|event| event.project().to_owned())
                .or_else(|| self.project.clone()),
            signals: polled.signals.clone(),
        }
    }

    /// Whether `event` is one it has not seen: an identity it has not seen,
    /// or the Release of a version it has seen only as a tag.
    #[must_use]
    pub fn unseen(&self, event: &Event) -> bool {
        let identity = event.identity();
        !self.seen.contains(identity)
            || (event.signal() == Some(Signal::Release) && self.tagged.contains(identity))
    }

    /// The validators the server last returned for `signal`'s request, where
    /// it returned any.
    #[must_use]
    pub fn validators(&self, signal: Signal) -> Option<&Validators> {
        self.signals.get(&signal)
    }
}

fn seen_in(found: &[Event], identity: &Identity) -> bool {
    found.iter().any(|event| event.identity() == identity)
}

/// The events of a poll to announce: those `cursor` has not seen, and each
/// Release of a version it has seen only as a tag. With no cursor, the
/// source is new, and only its newest `backfill` are announced.
#[must_use]
pub fn fresh(cursor: Option<&Cursor>, found: Vec<Event>, backfill: usize) -> Vec<Event> {
    match cursor {
        Some(cursor) => found
            .into_iter()
            .filter(|event| cursor.unseen(event))
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
        let tags = Validators {
            etag: Some("tags".to_owned()),
            last_modified: None,
        };
        let cursor = Cursor {
            seen: vec![Identity::new("b"), Identity::new("a")],
            etag: Some("old".to_owned()),
            signals: BTreeMap::from([(Signal::Release, Validators::default())]),
            ..Cursor::default()
        };
        let advanced = cursor.advanced(
            &Polled {
                events: vec![event("c"), event("b")],
                etag: Some("new".to_owned()),
                last_modified: Some("then".to_owned()),
                signals: BTreeMap::from([(Signal::Tag, tags.clone())]),
                passed: vec![Identity::new("b"), Identity::new("p"), Identity::new("a")],
            },
            500,
        );
        assert_eq!(
            advanced,
            Cursor {
                seen: vec![
                    Identity::new("c"),
                    Identity::new("b"),
                    Identity::new("p"),
                    Identity::new("a")
                ],
                etag: Some("new".to_owned()),
                last_modified: Some("then".to_owned()),
                project: Some(event("c").project().to_owned()),
                signals: BTreeMap::from([(Signal::Tag, tags.clone())]),
                tagged: vec![Identity::new("b"), Identity::new("p"), Identity::new("a")],
            }
        );
        assert_eq!(advanced.validators(Signal::Tag), Some(&tags));
        assert_eq!(advanced.validators(Signal::Release), None);
        let quiet = Cursor {
            project: Some("kept".to_owned()),
            ..Cursor::default()
        }
        .advanced(&Polled::default(), 500);
        assert_eq!(quiet.project.as_deref(), Some("kept"));
    }

    fn signalled(identity: &str, signal: Signal) -> Event {
        let mut attributes = crate::versioned("v1", false);
        attributes.insert(
            crate::SIGNAL.to_owned(),
            crate::Attribute::Text(signal.name().to_owned()),
        );
        Event::normalise(Draft {
            identity: identity.to_owned(),
            attributes,
            ..draft()
        })
        .unwrap()
    }

    #[test]
    fn a_release_of_a_version_seen_only_as_a_tag_is_unseen_until_it_is_seen() {
        let polled = |events: Vec<Event>| Polled {
            events,
            ..Polled::default()
        };
        let tagged = Cursor::default().advanced(
            &polled(vec![
                signalled("v2", Signal::Tag),
                signalled("v1", Signal::Release),
            ]),
            500,
        );
        assert_eq!(tagged.tagged, [Identity::new("v2")]);
        let release = signalled("v2", Signal::Release);
        assert!(tagged.unseen(&release));
        assert!(!tagged.unseen(&signalled("v2", Signal::Tag)));
        assert!(!tagged.unseen(&signalled("v1", Signal::Release)));
        assert!(tagged.unseen(&signalled("v3", Signal::Tag)));
        assert_eq!(
            identities(&fresh(
                Some(&tagged),
                vec![release.clone(), signalled("v1", Signal::Release)],
                0
            )),
            ["v2"]
        );
        let released = tagged.advanced(&polled(vec![release.clone()]), 500);
        assert_eq!(released.tagged, Vec::<Identity>::new());
        assert!(!released.unseen(&release));
        let quiet = tagged.advanced(&polled(Vec::new()), 500);
        assert_eq!(quiet.tagged, [Identity::new("v2")]);
        let forgotten = tagged.advanced(&polled(vec![signalled("v9", Signal::Release)]), 1);
        assert_eq!(forgotten.tagged, Vec::<Identity>::new());
        let twice = Cursor::default().advanced(
            &Polled {
                events: vec![signalled("v4", Signal::Tag)],
                passed: vec![Identity::new("v4")],
                ..Polled::default()
            },
            500,
        );
        assert_eq!(twice.tagged, [Identity::new("v4")]);
    }

    #[test]
    fn a_cursor_remembers_a_bounded_number_of_identities() {
        let found: Vec<Event> = (0..30).map(|index| event(&index.to_string())).collect();
        let advanced = Cursor::default().advanced(
            &Polled {
                events: found,
                ..Polled::default()
            },
            20,
        );
        assert_eq!(advanced.seen.len(), 20);
        assert_eq!(advanced.seen.first().map(Identity::as_str), Some("0"));
        assert_eq!(advanced.seen.last().map(Identity::as_str), Some("19"));
    }

    #[test]
    fn a_cursor_round_trips_through_json() {
        let cursor = Cursor {
            seen: vec![Identity::new("a")],
            etag: Some("e".to_owned()),
            ..Cursor::default()
        };
        let json = serde_json::to_string(&cursor).unwrap();
        assert_eq!(
            json,
            r#"{"seen":["a"],"etag":"e","last_modified":null,"project":null}"#
        );
        assert_eq!(serde_json::from_str::<Cursor>(&json).unwrap(), cursor);
        let tagged = Cursor {
            signals: BTreeMap::from([(
                Signal::Tag,
                Validators {
                    etag: Some("t".to_owned()),
                    last_modified: None,
                },
            )]),
            ..cursor
        };
        let json = serde_json::to_string(&tagged).unwrap();
        assert!(
            json.ends_with(r#""signals":{"tag":{"etag":"t","last_modified":null}}}"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Cursor>(&json).unwrap(), tagged);
    }
}

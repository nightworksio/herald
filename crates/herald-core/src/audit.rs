//! What each peer asked of this herald and how it was answered, kept per
//! peer and UTC day, and how many requests a peer made in its rate's
//! window (ADR-0054).

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::Action;

/// One request a peer made, or one signed by a key no peer holds: when,
/// by whom, what it asked of which resource, what it was answered and
/// why; never its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audited {
    /// When it came.
    pub at: Timestamp,
    /// The peer whose key signed it, where one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    /// The id of the key it named, where no peer holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyid: Option<String>,
    /// What it asked.
    pub action: Action,
    /// The resource it asked it of, such as `sources.releases`.
    pub resource: String,
    /// The status it was answered.
    pub outcome: u16,
    /// Why it was answered so.
    pub reason: String,
    /// What the ID token it came with said of the run that sent it, where
    /// it came with one: the token's issuer, its id and run where it names
    /// them, and each claim its app's entry binds; never the token.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<String, String>,
    /// The identity of the envelope it pushed with an ID token: as taken,
    /// under `herald/<peer>/` where its grant's namespaces hold it not;
    /// else as the envelope names it, where it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// The digest of the body it pushed with an ID token, as its
    /// `Content-Digest` names it: `sha-256=:<base64>:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl Audited {
    /// The most bytes a key id or resource a request names is kept with.
    pub const NAMED_MOST: usize = 256;

    /// `text` cut to at most [`Audited::NAMED_MOST`] bytes, on a character
    /// boundary.
    #[must_use]
    pub fn named(text: &str) -> String {
        text[..text.floor_char_boundary(Self::NAMED_MOST)].to_owned()
    }
}

/// What one peer, or every key no peer holds, asked on one UTC day.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AuditDay {
    /// The peer, or [`AuditDay::STRANGERS`].
    pub peer: String,
    /// The day, as `YYYY-MM-DD` in UTC.
    pub day: String,
    /// Its requests, oldest first, at most [`AuditDay::MOST`].
    pub entries: Vec<Audited>,
    /// How many more it made that are counted but not kept.
    #[serde(default)]
    pub more: u64,
}

impl AuditDay {
    /// The name the requests no peer's key signed are kept under, which no
    /// peer's name can be.
    pub const STRANGERS: &str = "-";
    /// The most requests one day keeps of one peer.
    pub const MOST: usize = 1000;

    /// An empty day of `peer`, the one `at` falls on.
    #[must_use]
    pub fn of(peer: &str, at: Timestamp) -> Self {
        Self {
            peer: peer.to_owned(),
            day: utc_day(at),
            entries: Vec::new(),
            more: 0,
        }
    }

    /// `entry` kept, or counted where the day keeps as many as it may.
    pub fn record(&mut self, entry: Audited) {
        if self.entries.len() < Self::MOST {
            self.entries.push(entry);
        } else {
            self.more = self.more.saturating_add(1);
        }
    }
}

/// The UTC day `at` falls on, as `YYYY-MM-DD`.
#[must_use]
pub fn utc_day(at: Timestamp) -> String {
    at.to_zoned(TimeZone::UTC).date().to_string()
}

/// How many requests a peer made in the window of its rate that began at
/// `start`; and, of a peer's own window, how many of its requests refused
/// past its rate have their nonces kept, and until when it is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Window {
    /// When the window began.
    pub start: Timestamp,
    /// How many requests it let through.
    pub count: u32,
    /// The requests refused past the rate whose nonces are kept, where any
    /// are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<Refusals>,
    /// Until when nothing of the peer is let through, where it was held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Timestamp>,
}

/// How many requests refused past a peer's rate have their nonces kept,
/// counted from `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusals {
    /// When the count began.
    pub start: Timestamp,
    /// How many have their nonces kept.
    pub count: u32,
}

/// What becomes of a request refused past a peer's rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusing {
    /// Its nonce is kept, so it is never taken later.
    Kept,
    /// Too many have their nonces kept: the peer is held, and nothing of
    /// it is taken before every request it made unkept has expired.
    Held,
}

impl Window {
    /// It after one more request at `at`, a window lasting `seconds`: a
    /// new window where this one is over. It counts past any rate, so the
    /// first request past one is told apart from the rest.
    #[must_use]
    pub fn counted(self, at: Timestamp, seconds: i64) -> Self {
        let window = if at.as_second() >= self.start.as_second().saturating_add(seconds)
            || at < self.start
        {
            Self {
                start: at,
                count: 0,
                ..self
            }
        } else {
            self
        };
        Self {
            count: window.count.saturating_add(1),
            ..window
        }
    }

    /// Whether the peer is held at `at`.
    #[must_use]
    pub fn holds(&self, at: Timestamp) -> bool {
        self.held.is_some_and(|held| at < held)
    }

    /// It holding the peer until `until` at the least.
    #[must_use]
    pub fn holding(self, until: Timestamp) -> Self {
        Self {
            held: Some(self.held.map_or(until, |held| held.max(until))),
            ..self
        }
    }

    /// It after a request refused past the rate at `at`, live until
    /// `until`: its nonce kept where fewer than `most` are, counted afresh
    /// once `seconds` passed since the count began; otherwise the peer held
    /// until `until`.
    #[must_use]
    pub fn refusing(
        self,
        at: Timestamp,
        (most, seconds): (u32, i64),
        until: Timestamp,
    ) -> (Self, Refusing) {
        let refused = self
            .refused
            .filter(|refused| {
                at >= refused.start
                    && at.as_second() < refused.start.as_second().saturating_add(seconds)
            })
            .unwrap_or(Refusals {
                start: at,
                count: 0,
            });
        if refused.count < most {
            let refused = Refusals {
                count: refused.count.saturating_add(1),
                ..refused
            };
            return (
                Self {
                    refused: Some(refused),
                    ..self
                },
                Refusing::Kept,
            );
        }
        (
            Self {
                refused: Some(refused),
                ..self.holding(until)
            },
            Refusing::Held,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).unwrap()
    }

    #[test]
    fn a_day_keeps_its_first_requests_and_counts_the_rest() {
        let mut kept = AuditDay::of("upstream", at(1_800_000_000));
        assert_eq!(kept.day, "2027-01-15");
        let entry = Audited {
            at: at(1_800_000_000),
            peer: Some("upstream".to_owned()),
            keyid: None,
            action: Action::Push,
            resource: "sources.releases".to_owned(),
            outcome: 202,
            reason: "taken".to_owned(),
            evidence: BTreeMap::new(),
            identity: None,
            digest: None,
        };
        for _ in 0..=AuditDay::MOST {
            kept.record(entry.clone());
        }
        kept.record(entry.clone());
        assert_eq!((kept.entries.len(), kept.more), (AuditDay::MOST, 2));
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("keyid"), "{json}");
        for absent in ["evidence", "identity", "digest"] {
            assert!(!json.contains(absent), "{json}");
        }
        assert_eq!(serde_json::from_str::<Audited>(&json).unwrap(), entry);
        let evidenced = Audited {
            evidence: BTreeMap::from([("iss".to_owned(), "https://issuer.example".to_owned())]),
            identity: Some("github.com/1/v1".to_owned()),
            digest: Some("sha-256=:YWI=:".to_owned()),
            ..entry.clone()
        };
        let json = serde_json::to_string(&evidenced).unwrap();
        assert!(
            json.contains(r#""evidence":{"iss":"https://issuer.example"},"identity":"github.com/1/v1","digest":"sha-256=:YWI=:""#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Audited>(&json).unwrap(), evidenced);
        assert_eq!(utc_day(at(86_399)), "1970-01-01");
        assert_eq!(utc_day(at(86_400)), "1970-01-02");
    }

    #[test]
    fn a_named_text_is_cut_on_a_character_boundary() {
        assert_eq!(Audited::named("short"), "short");
        let long = format!("{}é", "a".repeat(Audited::NAMED_MOST - 1));
        assert_eq!(Audited::named(&long), "a".repeat(Audited::NAMED_MOST - 1));
        assert_eq!(Audited::named(&"b".repeat(300)).len(), Audited::NAMED_MOST);
    }

    #[test]
    fn a_window_counts_each_request_until_it_is_over() {
        let first = Window {
            start: at(100),
            ..Window::default()
        };
        let two = first.counted(at(100), 60).counted(at(159), 60);
        assert_eq!(
            two,
            Window {
                start: at(100),
                count: 2,
                ..Window::default()
            }
        );
        assert_eq!(two.counted(at(100), 60).count, 3);
        assert_eq!(
            two.counted(at(160), 60),
            Window {
                start: at(160),
                count: 1,
                ..Window::default()
            }
        );
        assert_eq!(
            two.counted(at(99), 60),
            Window {
                start: at(99),
                count: 1,
                ..Window::default()
            }
        );
        let full = Window {
            start: at(100),
            count: u32::MAX,
            ..Window::default()
        };
        assert_eq!(full.counted(at(101), 60).count, u32::MAX);
        let held = Window {
            held: Some(at(500)),
            refused: Some(Refusals {
                start: at(100),
                count: 1,
            }),
            ..full
        };
        let reopened = held.counted(at(400), 60);
        assert_eq!((reopened.held, reopened.refused), (held.held, held.refused));
    }

    #[test]
    fn a_window_keeps_refused_nonces_up_to_its_most_then_holds_until_the_last_expires() {
        let open = Window::default();
        let (one, kept) = open.refusing(at(100), (2, 60), at(400));
        assert_eq!(kept, Refusing::Kept);
        assert_eq!(
            one.refused,
            Some(Refusals {
                start: at(100),
                count: 1
            })
        );
        let (again, kept) = one.refusing(at(100), (2, 60), at(400));
        assert_eq!((kept, again.refused.unwrap().count), (Refusing::Kept, 2));
        let (two, kept) = one.refusing(at(159), (2, 60), at(400));
        assert_eq!((kept, two.refused.unwrap().count), (Refusing::Kept, 2));
        let (held, first) = two.refusing(at(159), (2, 60), at(450));
        assert_eq!(first, Refusing::Held);
        assert_eq!(held.held, Some(at(450)));
        assert!(held.holds(at(449)) && !held.holds(at(450)));
        let (still, again) = held.refusing(at(159), (2, 60), at(420));
        assert_eq!(again, Refusing::Held);
        assert_eq!(still.held, Some(at(450)));
        let (afresh, kept) = held.refusing(at(160), (2, 60), at(500));
        assert_eq!(kept, Refusing::Kept);
        assert_eq!(
            afresh.refused,
            Some(Refusals {
                start: at(160),
                count: 1
            })
        );
        let (earlier, kept) = two.refusing(at(99), (2, 60), at(400));
        assert_eq!(
            (kept, earlier.refused.unwrap().start),
            (Refusing::Kept, at(99))
        );
        let (later, again) = held.refusing(at(500), (0, 60), at(600));
        assert_eq!(again, Refusing::Held);
        assert_eq!(later.held, Some(at(600)));
        assert_eq!(
            later.refused,
            Some(Refusals {
                start: at(500),
                count: 0
            })
        );
    }
}

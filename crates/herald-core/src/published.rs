//! The key set an instance last published, kept so its sequence rises by
//! one with each new set (ADR-0044), the envelopes each of its feeds
//! lists, the key set each peer last handed over to (ADR-0049), and the
//! static signed log it last wrote (ADR-0060).

use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// The key set a peer whose rotation is signed last proved with a key
/// trusted for it, kept in place of the keys pinned for it while those stay
/// pinned (ADR-0049).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerKeys {
    /// The keys pinned for the peer when the set was taken, each as its
    /// Multikey.
    pub pinned: Vec<String>,
    /// The set's sequence.
    pub sequence: u64,
    /// The keys, newest first, each as its Multikey.
    pub keys: Vec<String>,
    /// When the peer's discovery document was last read.
    pub read: Timestamp,
    /// How many seconds the set is kept before the document is read again.
    pub refresh: u64,
}

impl PeerKeys {
    /// Whether it stands for a peer for which `pinned` is pinned now: a set
    /// taken over other pinned keys stands for nothing.
    #[must_use]
    pub fn stands(&self, pinned: &[String]) -> bool {
        self.pinned == pinned
    }

    /// Whether the peer's discovery document is due to be read again at
    /// `now`.
    #[must_use]
    pub fn due(&self, now: Timestamp) -> bool {
        let refresh = SignedDuration::from_secs(i64::try_from(self.refresh).unwrap_or(i64::MAX));
        self.read.checked_add(refresh).is_ok_and(|next| now >= next)
    }
}

/// The static signed log an instance last wrote (ADR-0060): the sequence
/// it took, which no other writing takes again, when it was written, the
/// digest of each file it holds by path, and the commit of the branch
/// holding them, where it was written to one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Publication {
    /// Raised by one each time the log is written, or a writing begins.
    pub sequence: u64,
    /// When the log was last written; absent before the first.
    pub published: Option<Timestamp>,
    /// The digest of each file the log holds, by its path.
    pub files: BTreeMap<String, String>,
    /// The commit of the branch holding the files, where they were
    /// written to one.
    pub commit: Option<String>,
}

impl Publication {
    /// Whether a log holding `files` is to be written at `now`: where its
    /// files changed, or it was last written `refresh` or longer ago.
    #[must_use]
    pub fn due(
        &self,
        files: &BTreeMap<String, String>,
        now: Timestamp,
        refresh: SignedDuration,
    ) -> bool {
        self.files != *files
            || self.published.is_none_or(|published| {
                published.checked_add(refresh).is_ok_and(|next| now >= next)
            })
    }

    /// The record taking the next sequence for a writing about to begin,
    /// its files and commit still those last written.
    #[must_use]
    pub fn begun(&self) -> Self {
        Self {
            sequence: self.sequence.saturating_add(1),
            ..self.clone()
        }
    }
}

/// What one feed lists: its newest envelopes, newest first, each under a
/// sequence never used twice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Feed {
    /// The sequence the newest envelope ever listed took; 0 before the first.
    pub newest: u64,
    /// The envelopes, newest first.
    pub items: Vec<FeedItem>,
}

/// One envelope a feed lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedItem {
    /// Its sequence.
    pub sequence: u64,
    /// The envelope, proved, naming the identity of the event it carries.
    pub envelope: Value,
}

impl FeedItem {
    /// The identity of the event it carries.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        self.envelope.get(IDENTITY).and_then(Value::as_str)
    }
}

/// The member of an envelope naming the identity of its event.
const IDENTITY: &str = "identity";

impl Feed {
    /// The envelope it lists for the event `identity`, where it lists one.
    #[must_use]
    pub fn held(&self, identity: &str) -> Option<&FeedItem> {
        self.items
            .iter()
            .find(|item| item.identity() == Some(identity))
    }

    /// The feed with `envelope` listed first under the next sequence, any
    /// earlier envelope of its event left out, and at most `keep` envelopes
    /// listed.
    #[must_use]
    pub fn entered(mut self, envelope: Value, keep: usize) -> Self {
        let identity = envelope.get(IDENTITY).cloned();
        self.items
            .retain(|item| item.envelope.get(IDENTITY) != identity.as_ref());
        self.newest = self.newest.saturating_add(1);
        self.items.insert(
            0,
            FeedItem {
                sequence: self.newest,
                envelope,
            },
        );
        self.items.truncate(keep);
        self
    }

    /// The envelopes a page of at most `size` lists, newest first, before
    /// the sequence `before` where it names one, and whether older ones
    /// follow.
    #[must_use]
    pub fn page(&self, before: Option<u64>, size: usize) -> (Vec<&FeedItem>, bool) {
        let mut older = self
            .items
            .iter()
            .filter(|item| before.is_none_or(|before| item.sequence < before));
        let listed: Vec<&FeedItem> = older.by_ref().take(size).collect();
        (listed, older.next().is_some())
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

    #[test]
    fn a_peer_s_set_stands_while_its_pinned_keys_do_and_is_read_again_once_due() {
        let read = Timestamp::constant(1_800_000_000, 0);
        let held = PeerKeys {
            pinned: keys(&["z6Mka"]),
            sequence: 3,
            keys: keys(&["z6Mkb", "z6Mka"]),
            read,
            refresh: 60,
        };
        assert!(held.stands(&keys(&["z6Mka"])));
        assert!(!held.stands(&keys(&["z6Mkc"])));
        assert!(!held.due(Timestamp::constant(1_800_000_059, 0)));
        assert!(held.due(Timestamp::constant(1_800_000_060, 0)));
        let forever = PeerKeys {
            refresh: u64::MAX,
            ..held
        };
        assert!(!forever.due(Timestamp::MAX));
    }

    #[test]
    fn a_log_is_due_where_its_files_changed_or_once_refresh_passed() {
        let files = BTreeMap::from([("a".to_owned(), "1".to_owned())]);
        let at = Timestamp::constant(1_800_000_000, 0);
        let refresh = SignedDuration::from_secs(60);
        assert!(Publication::default().due(&BTreeMap::new(), at, refresh));
        let written = Publication {
            sequence: 3,
            published: Some(at),
            files: files.clone(),
            commit: Some("c".to_owned()),
        };
        assert!(!written.due(&files, Timestamp::constant(1_800_000_059, 0), refresh));
        assert!(written.due(&files, Timestamp::constant(1_800_000_060, 0), refresh));
        assert!(written.due(&BTreeMap::new(), at, refresh));
        let never = Publication {
            published: Some(Timestamp::MAX),
            ..written.clone()
        };
        assert!(!never.due(&files, Timestamp::MAX, refresh));
        assert_eq!(
            written.begun(),
            Publication {
                sequence: 4,
                ..written
            }
        );
    }

    fn feed(identities: &[&str]) -> Feed {
        identities.iter().fold(Feed::default(), |feed, identity| {
            feed.entered(serde_json::json!({ "identity": identity }), 3)
        })
    }

    fn sequences(listed: &[&FeedItem]) -> Vec<u64> {
        listed.iter().map(|item| item.sequence).collect()
    }

    #[test]
    fn a_feed_lists_the_newest_first_under_sequences_never_used_twice() {
        let fed = feed(&["a", "b", "c", "d"]);
        assert_eq!(fed.newest, 4);
        assert_eq!(sequences(&fed.page(None, 10).0), [4, 3, 2]);
        let again = fed.entered(serde_json::json!({ "identity": "c", "n": 2 }), 3);
        assert_eq!(again.newest, 5);
        assert_eq!(sequences(&again.page(None, 10).0), [5, 4, 2]);
        assert_eq!(
            again.held("c").map(|item| (item.sequence, &item.envelope)),
            Some((5, &serde_json::json!({ "identity": "c", "n": 2 })))
        );
        assert_eq!(again.held("a"), None);
        assert_eq!(again.items[0].identity(), Some("c"));
        let unnamed = FeedItem {
            sequence: 1,
            envelope: serde_json::json!({ "identity": 7 }),
        };
        assert_eq!(unnamed.identity(), None);
    }

    #[test]
    fn a_page_lists_what_is_before_its_cursor_and_says_whether_more_follow() {
        let fed = ["a", "b", "c", "d", "e"]
            .iter()
            .fold(Feed::default(), |feed, identity| {
                feed.entered(serde_json::json!({ "identity": identity }), 10)
            });
        let (first, more) = fed.page(None, 2);
        assert_eq!((sequences(&first), more), (vec![5, 4], true));
        let (next, more) = fed.page(Some(4), 2);
        assert_eq!((sequences(&next), more), (vec![3, 2], true));
        let (last, more) = fed.page(Some(2), 2);
        assert_eq!((sequences(&last), more), (vec![1], false));
        let (exact, more) = fed.page(Some(3), 2);
        assert_eq!((sequences(&exact), more), (vec![2, 1], false));
        assert_eq!(fed.page(Some(1), 2), (Vec::new(), false));
    }
}

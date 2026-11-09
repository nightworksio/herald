/// The markup a destination renders, as data: how its text is escaped,
/// how links, headings, emphasis, code and list items are written, and
/// whether HTML is written beside a plain body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Markup {
    /// How text is escaped, and inline code written.
    pub escape: Escape,
    /// How a link is written.
    pub links: Links,
    /// How a heading is written.
    pub heading: Heading,
    /// The marks around emphasis, strong text and struck text.
    pub marks: Emphases,
    /// How a code block is fenced.
    pub fences: Fences,
    /// How a list item without a number starts.
    pub bullet: Bullet,
    /// Whether HTML is written beside the plain body.
    pub html: bool,
    /// The prefix of links that mention rather than lead somewhere, which
    /// are never kept as links.
    pub pills: Option<&'static str>,
}

/// How text is escaped, and inline code written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    /// Every ASCII punctuation mark but `@` behind a backslash, each sigil
    /// broken from what follows; inline code fenced in backticks.
    Backslash,
    /// `&`, `<` and `>` written as entities; inline code fenced in one
    /// backtick where it holds none.
    Entities,
    /// Nothing escaped and nothing fenced: every mention and markup
    /// character neutralised.
    Neutral,
}

/// How a link is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Links {
    /// Its text, then its target in parentheses and angle brackets.
    Bracketed,
    /// Its target and its text in angle brackets, split by a pipe.
    Piped,
    /// Its text, then its target spelled out in parentheses.
    Spelled,
}

/// How a heading is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heading {
    /// Behind as many hashes as its level, at most this many.
    Hashes(u8),
    /// Between these marks.
    Wrapped(&'static str),
    /// As plain text.
    Bare,
}

/// The marks around emphasis, strong text and struck text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Emphases {
    /// Around emphasis.
    pub emphasis: &'static str,
    /// Around strong text.
    pub strong: &'static str,
    /// Around struck text.
    pub strike: &'static str,
}

/// How a code block is fenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fences {
    /// In backticks, naming its language.
    Labelled,
    /// In backticks.
    Bare,
    /// Not at all.
    None,
}

/// How a list item without a number starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bullet {
    /// With this marker, whatever the symbols say.
    Fixed(&'static str),
    /// With the symbols' bullet, else this one.
    Symbol(char),
}

/// How a destination counts the length of a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// In characters.
    Chars,
    /// In bytes of UTF-8.
    Bytes,
}

/// How long the parts of an announcement may be on a destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The longest title.
    pub title: usize,
    /// The longest text of the announcement itself.
    pub text: usize,
    /// The longest text of each overflow message after it.
    pub followup: usize,
    /// The longest thread name.
    pub thread_name: usize,
}

/// How a destination threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Threads {
    /// It has no threads.
    None,
    /// A thread is opened from a message, as a step of its own.
    FromMessage,
    /// A reply to a message is its thread, with no step to open it.
    Replies,
}

/// How a destination pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pins {
    /// It pins nothing.
    None,
    /// It holds many pins, so the new one is pinned before the route's
    /// earlier one is unpinned.
    Many,
    /// It holds one pin, so the route's earlier one is unpinned before the
    /// new one is pinned.
    One,
}

/// How a destination mentions people.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionStyle {
    /// It mentions no one.
    None,
    /// It pings roles the platform keeps.
    Roles,
    /// It mentions users, from the subscribers herald keeps, as many in a
    /// message as its room holds.
    Users(MentionRoom),
}

/// How many mentions of users a destination's messages hold, in bytes: the
/// announcement keeps `first` for them and each message after it `rest`, and
/// a mention writes its user id `copies` times with `overhead` more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MentionRoom {
    /// The bytes the announcement keeps for its mentions.
    pub first: usize,
    /// The bytes each message of mentions after it holds.
    pub rest: usize,
    /// How many times a mention writes its user id.
    pub copies: usize,
    /// The bytes a mention writes beyond its user id's copies.
    pub overhead: usize,
}

impl MentionRoom {
    /// The bytes mentioning `user` takes.
    #[must_use]
    pub fn cost(&self, user: &str) -> usize {
        self.copies
            .saturating_mul(user.len())
            .saturating_add(self.overhead)
    }

    /// `users` spread over messages: the announcement's first, then as many
    /// messages as the rest need, each within its room. A mention too long
    /// for any room takes a message alone.
    #[must_use]
    pub fn spread(&self, users: Vec<String>) -> Vec<Vec<String>> {
        let mut messages = Vec::new();
        let mut current = Vec::new();
        let mut left = self.first;
        for user in users {
            let cost = self.cost(&user);
            if cost > left && (!current.is_empty() || messages.is_empty()) {
                messages.push(std::mem::take(&mut current));
                left = self.rest;
            }
            left = left.saturating_sub(cost);
            current.push(user);
        }
        messages.push(current);
        messages
    }
}

/// What a destination can do beyond posting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Features {
    /// How it threads.
    pub threads: Threads,
    /// Whether it crossposts.
    pub crosspost: bool,
    /// How it pins.
    pub pins: Pins,
    /// Whether herald can read back its own posts, for destination markers.
    pub reads_back: bool,
    /// Whether it edits a post.
    pub edits: bool,
    /// How it mentions.
    pub mentions: MentionStyle,
    /// Whether it sends interactions to an endpoint.
    pub interactions: bool,
}

/// A destination's markup, limits and features, as the core plans with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// The markup it renders.
    pub markup: Markup,
    /// How it counts length.
    pub measure: Measure,
    /// How long each part may be.
    pub limits: Limits,
    /// What it can do beyond posting.
    pub features: Features,
    /// The characters it writes where its markup has none.
    pub symbols: crate::symbols::Symbols,
}

impl Features {
    /// The options of `delivery` a destination with these features cannot do,
    /// by the key a route writes each under. Archiving and locking need a
    /// thread opened from a message, as a thread of replies is not one to
    /// archive. A destination whose every post opens a thread of its own, as
    /// a Discord forum's does, takes `thread` whatever this says.
    #[must_use]
    pub fn lacking(&self, delivery: &crate::Delivery) -> Vec<&'static str> {
        let threads = self.threads != Threads::None;
        let kept = self.threads == Threads::FromMessage;
        let thread = delivery.thread.as_ref();
        [
            (delivery.crosspost && !self.crosspost, "crosspost"),
            (thread.is_some() && !threads, "thread"),
            (
                thread.is_some_and(|thread| thread.archive_after.is_some()) && !kept,
                "thread.archive_after",
            ),
            (
                thread.is_some_and(|thread| thread.lock_after.is_some()) && !kept,
                "thread.lock_after",
            ),
            (delivery.pin.is_some() && self.pins == Pins::None, "pin"),
            (
                !delivery.mentions.roles.is_empty() && self.mentions != MentionStyle::Roles,
                "mentions.roles",
            ),
            (
                delivery.mentions.subscribers && !matches!(self.mentions, MentionStyle::Users(_)),
                "mentions.subscribers",
            ),
        ]
        .into_iter()
        .filter_map(|(lacks, key)| lacks.then_some(key))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::SignedDuration;

    use crate::{Delivery, Mentions, Pinning, RoleId, Thread};

    const ROOM: MentionRoom = MentionRoom {
        first: 10,
        rest: 20,
        copies: 2,
        overhead: 1,
    };

    fn users(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn mentions_spread_over_messages_each_within_its_room() {
        assert_eq!(ROOM.cost("abc"), 7);
        assert_eq!(ROOM.spread(Vec::new()), [Vec::<String>::new()]);
        assert_eq!(
            ROOM.spread(users(&["ab", "cd", "ef", "gh", "ij", "kl"])),
            [users(&["ab", "cd"]), users(&["ef", "gh", "ij", "kl"])]
        );
        assert_eq!(
            ROOM.spread(users(&["abcdefgh", "ab"])),
            [Vec::<String>::new(), users(&["abcdefgh"]), users(&["ab"])]
        );
        assert_eq!(
            ROOM.spread(users(&["a".repeat(30).as_str(), "ab"])),
            [
                Vec::<String>::new(),
                users(&["a".repeat(30).as_str()]),
                users(&["ab"])
            ]
        );
    }

    fn features(mentions: MentionStyle) -> Features {
        Features {
            threads: Threads::None,
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: false,
            mentions,
            interactions: false,
        }
    }

    fn asking(roles: &[&str], subscribers: bool) -> Delivery {
        Delivery {
            crosspost: false,
            thread: None,
            pin: None,
            mentions: Mentions {
                roles: roles.iter().map(|role| RoleId::new(*role)).collect(),
                subscribers,
            },
        }
    }

    #[test]
    fn a_delivery_asking_nothing_lacks_nothing_anywhere() {
        for style in [
            MentionStyle::None,
            MentionStyle::Roles,
            MentionStyle::Users(ROOM),
        ] {
            assert_eq!(
                features(style).lacking(&asking(&[], false)),
                Vec::<&str>::new()
            );
        }
    }

    #[test]
    fn roles_are_lacking_only_where_the_destination_mentions_no_roles() {
        let roles = asking(&["1"], false);
        assert_eq!(
            features(MentionStyle::Roles).lacking(&roles),
            Vec::<&str>::new()
        );
        assert_eq!(
            features(MentionStyle::Users(ROOM)).lacking(&roles),
            ["mentions.roles"]
        );
        assert_eq!(
            features(MentionStyle::None).lacking(&roles),
            ["mentions.roles"]
        );
    }

    #[test]
    fn subscribers_are_lacking_only_where_the_destination_mentions_no_users() {
        let subscribers = asking(&[], true);
        assert_eq!(
            features(MentionStyle::Users(ROOM)).lacking(&subscribers),
            Vec::<&str>::new()
        );
        assert_eq!(
            features(MentionStyle::Roles).lacking(&subscribers),
            ["mentions.subscribers"]
        );
    }

    #[test]
    fn crossposting_and_pinning_are_lacking_where_the_destination_cannot() {
        let delivery = Delivery {
            crosspost: true,
            pin: Some(Pinning::Markers),
            ..asking(&[], false)
        };
        assert_eq!(
            features(MentionStyle::Roles).lacking(&delivery),
            ["crosspost", "pin"]
        );
        let able = Features {
            crosspost: true,
            pins: Pins::Many,
            ..features(MentionStyle::Roles)
        };
        assert_eq!(able.lacking(&delivery), Vec::<&str>::new());
    }

    #[test]
    fn a_thread_is_lacking_where_the_destination_has_none_and_its_chores_where_it_keeps_none() {
        let after = Some(SignedDuration::from_hours(24));
        let delivery = Delivery {
            thread: Some(Thread {
                name: "{{ event.version }}".to_owned(),
                archive_after: after,
                lock_after: after,
            }),
            ..asking(&[], false)
        };
        assert_eq!(
            features(MentionStyle::Roles).lacking(&delivery),
            ["thread", "thread.archive_after", "thread.lock_after"]
        );
        let threaded = Features {
            threads: Threads::FromMessage,
            ..features(MentionStyle::Roles)
        };
        assert_eq!(threaded.lacking(&delivery), Vec::<&str>::new());
        let replied = Features {
            threads: Threads::Replies,
            ..features(MentionStyle::Roles)
        };
        assert_eq!(
            replied.lacking(&delivery),
            ["thread.archive_after", "thread.lock_after"]
        );
        let named_only = Delivery {
            thread: Some(Thread {
                name: "{{ event.version }}".to_owned(),
                archive_after: None,
                lock_after: None,
            }),
            ..asking(&[], false)
        };
        assert_eq!(
            features(MentionStyle::Roles).lacking(&named_only),
            ["thread"]
        );
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Each destination's markup as its own crate writes it, for the core's
    //! tests.

    use super::{Bullet, Emphases, Escape, Fences, Heading, Links, Markup};

    pub(crate) const DISCORD: Markup = Markup {
        escape: Escape::Backslash,
        links: Links::Bracketed,
        heading: Heading::Hashes(3),
        marks: Emphases {
            emphasis: "*",
            strong: "**",
            strike: "~~",
        },
        fences: Fences::Labelled,
        bullet: Bullet::Fixed("- "),
        html: false,
        pills: None,
    };

    pub(crate) const SLACK: Markup = Markup {
        escape: Escape::Entities,
        links: Links::Piped,
        heading: Heading::Wrapped("*"),
        marks: Emphases {
            emphasis: "_",
            strong: "*",
            strike: "~",
        },
        fences: Fences::Bare,
        bullet: Bullet::Symbol('•'),
        html: false,
        pills: None,
    };

    pub(crate) const PLAIN: Markup = Markup {
        escape: Escape::Neutral,
        links: Links::Spelled,
        heading: Heading::Bare,
        marks: Emphases {
            emphasis: "",
            strong: "",
            strike: "",
        },
        fences: Fences::None,
        bullet: Bullet::Symbol('-'),
        html: false,
        pills: None,
    };

    pub(crate) const MATRIX: Markup = Markup {
        html: true,
        pills: Some("https://matrix.to/"),
        ..PLAIN
    };
}

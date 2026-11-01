/// The markup a destination renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Discord's Markdown.
    Discord,
    /// Slack's `mrkdwn`.
    Slack,
    /// Matrix's HTML, beside a plain body.
    Matrix,
    /// Plain text, as Mastodon shows it.
    Plain,
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
    /// It mentions users, from the subscribers herald keeps.
    Users,
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

/// A destination's dialect, limits and features, as the core plans with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// The markup it renders.
    pub dialect: Dialect,
    /// How it counts length.
    pub measure: Measure,
    /// How long each part may be.
    pub limits: Limits,
    /// What it can do beyond posting.
    pub features: Features,
}

impl Features {
    /// The options of `delivery` a destination with these features cannot do,
    /// by the key a route writes each under.
    #[must_use]
    pub fn lacking(&self, delivery: &crate::Delivery) -> Vec<&'static str> {
        [
            (delivery.crosspost && !self.crosspost, "crosspost"),
            (delivery.pin.is_some() && self.pins == Pins::None, "pin"),
            (
                !delivery.mentions.roles.is_empty() && self.mentions != MentionStyle::Roles,
                "mentions.roles",
            ),
            (
                delivery.mentions.subscribers && self.mentions != MentionStyle::Users,
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
    use crate::{Delivery, Mentions, Pinning, RoleId};

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
        for style in [MentionStyle::None, MentionStyle::Roles, MentionStyle::Users] {
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
            features(MentionStyle::Users).lacking(&roles),
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
            features(MentionStyle::Users).lacking(&subscribers),
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
}

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
    /// Whether it pins.
    pub pins: bool,
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

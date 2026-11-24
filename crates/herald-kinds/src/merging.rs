//! What a forge source gives a merge queue on one of its repositories, or
//! why it takes none as asked (ADR-0087).

use herald_core::merging::{Method, Update, Write};

/// What a merge queue asks of the source it names.
#[derive(Debug, Clone, Copy)]
pub struct MergeAsked<'a> {
    /// The repository it merges in, where it names one.
    pub repository: Option<&'a str>,
    /// How it merges, where it says.
    pub method: Option<Method>,
    /// Whether it re-runs failed checks.
    pub retries: bool,
    /// The credential its steps are made with, where it names one.
    pub write: Option<&'a Write>,
}

/// Why a source takes no merge queue as asked: the queue's key at fault,
/// and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmerged {
    /// The key, as the queue's table writes it.
    pub key: &'static str,
    /// Why, with the fix.
    pub why: String,
}

/// What a forge source gives a merge queue on one of its repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merging {
    /// The source's kind, as the config names it.
    pub forge: &'static str,
    /// The repository's full path.
    pub repository: String,
    /// The base URL of the forge's API, the one origin a step goes to.
    pub api: String,
    /// How a pull request's branch is brought up to date.
    pub update: Update,
    /// Why the forge has no merge queue of its own, where it never has.
    pub queueless: Option<&'static str>,
}

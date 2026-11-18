use std::fmt;

use herald_ports::{Boxed, DynHttp};

use crate::Unpolled;

/// The most open pull requests a repository's listing reads by default.
pub const MOST_OPEN: usize = 500;

/// The most open pull requests of a repository read in full by default,
/// where each takes requests of its own.
pub const MOST_PULLS: usize = 100;

/// How many open pull requests of a repository a view reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Most {
    /// The most it lists.
    pub open: usize,
    /// The most it reads in full, where each takes requests of its own;
    /// those past it read as unknown.
    pub pulls: usize,
}

impl Default for Most {
    fn default() -> Self {
        Self {
            open: MOST_OPEN,
            pulls: MOST_PULLS,
        }
    }
}

/// What reading a source's open pull requests costs each round, as doctor
/// shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueCost {
    /// How they are read, such as `graphql`, `listing` or `rest`.
    pub path: &'static str,
    /// The most requests a round takes.
    pub requests: usize,
    /// Whether it reads whether each can be merged; a batch of bumps counts
    /// none green where it does not.
    pub mergeability: bool,
}

/// A source's repository's open pull requests, read for a view.
pub trait Queue: fmt::Debug + Send + Sync {
    /// Its open pull requests, at most `most`, asked through `http`.
    fn opened<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        most: Most,
    ) -> Boxed<'a, Result<Vec<herald_core::Pull>, Unpolled>>;
}

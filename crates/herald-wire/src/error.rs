use std::fmt;

use herald_core::Problem;

/// Why what a source was handed could not become events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The input is larger than a parser reads.
    TooLarge {
        /// What was read.
        what: &'static str,
        /// The most bytes read.
        limit: usize,
    },
    /// The input is not what the source sends.
    Unreadable {
        /// What was read.
        what: &'static str,
        /// Where and why it could not be read.
        reason: String,
    },
    /// The input reads, but makes no event.
    Invalid {
        /// What was read.
        what: &'static str,
        /// Why it makes no event.
        problem: Problem,
    },
    /// The source answered with a status that is not an answer.
    Answered {
        /// What was asked for.
        what: &'static str,
        /// The status.
        status: u16,
    },
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { what, limit } => {
                write!(f, "{what} is larger than the {limit} bytes herald reads")
            }
            Self::Unreadable { what, reason } => write!(f, "{what} could not be read: {reason}"),
            Self::Invalid { what, problem } => write!(f, "{what} makes no event: {problem}"),
            Self::Answered { what, status } => write!(f, "{what} was answered with {status}"),
        }
    }
}

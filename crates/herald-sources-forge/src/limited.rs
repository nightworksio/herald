//! A forge's answer that the rate limit on its token or address was
//! reached, told apart from a refusal: GitHub sends some as 403, which
//! would otherwise read as a token without the permission.

use herald_core::status::{FORBIDDEN, OK, TOO_MANY_REQUESTS};
use herald_kinds::Unpolled;
use herald_ports::Response;
use herald_wire::SourceError;
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::Forge;

/// What GitHub's error body says, by its documented shape.
#[derive(Deserialize)]
struct Said {
    message: String,
}

/// The errors a GraphQL answer carries.
#[derive(Deserialize)]
struct Erred {
    #[serde(default)]
    errors: Vec<IgnoredAny>,
}

/// The header naming the requests left before `forge`'s rate limit, and
/// the one naming when it resets in seconds since the epoch, where
/// `forge` sends them.
const fn headers(forge: Forge) -> Option<(&'static str, &'static str)> {
    match forge {
        Forge::GitHub => Some(("x-ratelimit-remaining", "x-ratelimit-reset")),
        Forge::GitLab => Some(("ratelimit-remaining", "ratelimit-reset")),
        Forge::Forgejo => None,
    }
}

/// Whether `answer` says `forge`'s rate limit was reached: a 429 on every
/// forge, and a 403 asking to retry after a while, saying no request is
/// left, or on GitHub saying in its message that a rate limit was
/// exceeded, as GitHub, GitHub Enterprise Server and GHE.com send their
/// primary and secondary limits; and GitHub's GraphQL answer of 200 that
/// carries errors where it asks to retry after a while or says no request
/// is left.
fn reached(forge: Forge, answer: &Response) -> bool {
    let waits = || {
        answer.header("retry-after").is_some()
            || headers(forge)
                .and_then(|(left, _)| answer.header(left))
                .is_some_and(|left| left.trim() == "0")
    };
    match answer.status {
        TOO_MANY_REQUESTS => true,
        FORBIDDEN => {
            let said = forge == Forge::GitHub
                && serde_json::from_slice::<Said>(&answer.body)
                    .is_ok_and(|said| said.message.to_ascii_lowercase().contains("rate limit"));
            said || waits()
        }
        OK => {
            forge == Forge::GitHub
                && serde_json::from_slice::<Erred>(&answer.body)
                    .is_ok_and(|erred| !erred.errors.is_empty())
                && waits()
        }
        _ => false,
    }
}

/// How long until `forge`'s rate limit that `answer` names resets: its
/// `Retry-After`, else its reset header measured from its own `Date`.
fn resets(forge: Forge, answer: &Response) -> Option<SignedDuration> {
    answer.retry_after().or_else(|| {
        let (_, reset) = headers(forge)?;
        let second = answer.header(reset)?.trim().parse::<i64>().ok()?;
        answer.until(Timestamp::from_second(second).ok()?)
    })
}

/// What `forge` said in `answer` to a read of `what`, where it says the
/// forge's rate limit was reached: with when it resets, where the forge
/// said.
fn said(forge: Forge, what: &str, answer: &Response) -> Option<String> {
    reached(forge, answer).then(|| {
        let when = resets(forge, answer).map_or_else(
            || "it did not say when the limit resets".to_owned(),
            |wait| format!("the limit resets in {wait:#}"),
        );
        format!("{what} could not be read: the forge's rate limit was reached; {when}")
    })
}

/// The stop of a read of `what` that `forge` answered with `answer`, where
/// it says the forge's rate limit was reached: with when it resets, where
/// the forge said.
pub(crate) fn limited(forge: Forge, what: &str, answer: &Response) -> Option<Unpolled> {
    said(forge, what, answer).map(Unpolled::RateLimited)
}

/// Whether `answer` to a read of `what` on `forge` is an answer, as a
/// source's error: a rate limit reached is said as one, and any other
/// failure by its status.
///
/// # Errors
///
/// Where it did not succeed.
pub(crate) fn answered(
    forge: Forge,
    what: &'static str,
    answer: &Response,
) -> Result<(), SourceError> {
    if let Some(reason) = said(forge, what, answer) {
        return Err(SourceError::RateLimited { reason });
    }
    herald_wire::answered(what, answer)
}

/// Why a poll stops on `error`: a rate limit reached stops it as one, and
/// anything else as an answer herald cannot read.
pub(crate) fn unread(error: SourceError) -> Unpolled {
    match error {
        SourceError::RateLimited { reason } => Unpolled::RateLimited(reason),
        error => Unpolled::unreadable(error),
    }
}

/// Whether `answer` to a read of `what` on `forge` is an answer: a rate
/// limit reached stops the read as one, and any other failure as an
/// answer herald cannot read.
///
/// # Errors
///
/// Where it did not succeed.
pub(crate) fn checked(forge: Forge, what: &'static str, answer: &Response) -> Result<(), Unpolled> {
    answered(forge, what, answer).map_err(unread)
}

#[cfg(test)]
mod tests;

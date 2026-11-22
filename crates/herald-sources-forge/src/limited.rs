//! A forge's answer that the rate limit on its token or address was
//! reached, told apart from a refusal: GitHub sends some as 403, which
//! would otherwise read as a token without the permission.

use herald_core::status::{FORBIDDEN, TOO_MANY_REQUESTS};
use herald_kinds::Unpolled;
use herald_ports::Response;
use herald_wire::answered;
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;

use crate::Forge;

/// What GitHub's error body says, by its documented shape.
#[derive(Deserialize)]
struct Said {
    message: String,
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
/// primary and secondary limits.
fn reached(forge: Forge, answer: &Response) -> bool {
    match answer.status {
        TOO_MANY_REQUESTS => true,
        FORBIDDEN => {
            let spent = headers(forge)
                .and_then(|(left, _)| answer.header(left))
                .is_some_and(|left| left.trim() == "0");
            let said = forge == Forge::GitHub
                && serde_json::from_slice::<Said>(&answer.body)
                    .is_ok_and(|said| said.message.to_ascii_lowercase().contains("rate limit"));
            answer.header("retry-after").is_some() || spent || said
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

/// The stop of a read of `what` that `forge` answered with `answer`, where
/// it says the forge's rate limit was reached: with when it resets, where
/// the forge said.
pub(crate) fn limited(forge: Forge, what: &str, answer: &Response) -> Option<Unpolled> {
    reached(forge, answer).then(|| {
        let when = resets(forge, answer).map_or_else(
            || "it did not say when the limit resets".to_owned(),
            |wait| format!("the limit resets in {wait:#}"),
        );
        Unpolled::RateLimited(format!(
            "the forge's {what} was refused for its rate limit; {when}"
        ))
    })
}

/// Whether `answer` to a read of `what` on `forge` is an answer: a rate
/// limit reached stops the read as one, and any other failure as an
/// answer herald cannot read.
///
/// # Errors
///
/// Where it did not succeed.
pub(crate) fn checked(forge: Forge, what: &'static str, answer: &Response) -> Result<(), Unpolled> {
    if let Some(limited) = limited(forge, what, answer) {
        return Err(limited);
    }
    answered(what, answer).map_err(Unpolled::unreadable)
}

#[cfg(test)]
mod tests;

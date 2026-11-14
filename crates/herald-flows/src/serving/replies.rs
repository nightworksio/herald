//! What herald answers a request: its replies in words, its refusals, each
//! with the reason kept for the log (ADR-0007), and the work a reply
//! leaves.

use herald_core::Scheme;
use herald_core::{media, status};
use herald_ports::{Store, StoreError};

use super::jobs::Job;
use crate::verification::{Rejected, Verified, released};

/// What herald answers a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The status.
    pub status: u16,
    /// What the body is, as `Content-Type` names it.
    pub kind: &'static str,
    /// The body.
    pub body: Vec<u8>,
    /// How many seconds the sender is asked to wait before it asks again,
    /// as `Retry-After` names them.
    pub retry_after: Option<u32>,
}

/// A request's reply, and the work it leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The reply.
    pub reply: Reply,
    /// The work to do once the reply is sent.
    pub job: Option<Job>,
    /// Why the request was refused, for the log where herald runs; its
    /// reply names no reason (ADR-0007).
    pub refused: Option<String>,
}

/// The reply `status` in words: `text`, as plain text.
pub(crate) fn text_reply(status: u16, text: &str) -> Answered {
    replied(status, media::TEXT, text)
}

/// The refusal of a request admitted as `verified` whose body could not be
/// read for `reason`. Its delivery is released first, so the sender's next
/// delivery of it is admitted, not refused as a replay (ADR-0013).
pub(crate) async fn unread<S: Store>(
    store: &S,
    scheme: Scheme,
    verified: &Verified,
    reason: impl std::fmt::Display,
) -> Answered {
    let reason = match released(store, scheme, verified).await {
        Ok(()) => reason.to_string(),
        Err(error) => format!("{reason}; {}", unreleased(&error)),
    };
    refusal(status::BAD_REQUEST, reason)
}

/// What a delivery that could not be released for `error` is logged as.
pub(super) fn unreleased(error: &StoreError) -> String {
    format!("its delivery could not be released: {error}")
}

/// The refusal of a request, answered `status` with herald's fixed body for
/// it, which names no reason and is the same for every refusal of its kind,
/// and `reason` kept for the log (ADR-0007).
pub fn refusal(status: u16, reason: impl Into<String>) -> Answered {
    let body = match status {
        status::CONFLICT => "herald has taken this request already",
        status::SERVER_ERROR.. => "herald cannot answer this request now",
        _ => "herald does not take this request",
    };
    Answered {
        refused: Some(reason.into()),
        ..text_reply(status, body)
    }
}

/// The refusal of a request whose work found no place, asking its sender
/// to wait `after`, in whole seconds rounded up.
#[must_use]
pub fn busy(after: jiff::SignedDuration) -> Answered {
    let refused = refusal(
        status::UNAVAILABLE,
        "the queue has no place for the work it leaves",
    );
    let whole = after
        .as_secs()
        .saturating_add(i64::from(after.subsec_nanos() > 0));
    Answered {
        reply: Reply {
            retry_after: Some(u32::try_from(whole.max(1)).unwrap_or(u32::MAX)),
            ..refused.reply
        },
        ..refused
    }
}

pub(crate) fn replied(status: u16, kind: &'static str, body: impl Into<Vec<u8>>) -> Answered {
    Answered {
        reply: Reply {
            status,
            kind,
            body: body.into(),
            retry_after: None,
        },
        job: None,
        refused: None,
    }
}

pub(crate) fn missing(what: &str) -> Answered {
    text_reply(status::NOT_FOUND, &format!("herald serves no {what} here"))
}

pub(crate) fn unmethodical() -> Answered {
    text_reply(
        status::METHOD_NOT_ALLOWED,
        "herald does not answer this method here",
    )
}

/// The reply to a request `rejected` keeps from its parser.
pub(crate) fn rejection(rejected: &Rejected) -> Answered {
    let status = match rejected {
        Rejected::Replayed => status::CONFLICT,
        Rejected::Unadmitted { .. } | Rejected::Unkept { .. } => status::UNAVAILABLE,
        _ => status::UNAUTHORIZED,
    };
    refusal(status, rejected.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_request_is_answered_by_why() {
        assert_eq!(rejection(&Rejected::Replayed).reply.status, 409);
        assert_eq!(
            rejection(&Rejected::Unadmitted {
                reason: "down".to_owned()
            })
            .reply
            .status,
            503
        );
        assert_eq!(rejection(&Rejected::Forged).reply.status, 401);
        let unkept = rejection(&Rejected::Unkept {
            reason: "down".to_owned(),
        });
        assert_eq!(
            (unkept.reply.status, unkept.refused.as_deref()),
            (
                503,
                Some(
                    "the keys activities are checked with could not be read or kept, so the activity is not let through: down"
                )
            )
        );
    }
}

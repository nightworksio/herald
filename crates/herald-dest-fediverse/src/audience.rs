//! Who may read a Mastodon route's followers-only statuses, asked right
//! before a run's first advisories section is sent there: only an account
//! that approves each follower is private (ADR-0067).

use herald_kinds::{Audience, UNADVISED};
use herald_ports::Http;
use serde_json::Value;

use crate::mastodon::Mastodon;
use crate::options::Visibility;
use crate::request::get;

/// What herald asks before a run's first advisory send, as doctor names
/// it.
pub const ASKS: &str = "the server's verify_credentials whether the account approves each follower (locked), through its token";

/// The requests the check takes.
pub(crate) const REQUESTS: usize = 1;

/// Who may read the statuses `mastodon`'s route posts, asked through
/// `http`.
pub(crate) async fn audience<H: Http>(mastodon: &Mastodon, http: &H) -> Audience {
    if mastodon.route.visibility != Visibility::Private {
        return Audience::Public(UNADVISED.to_owned());
    }
    let answer = match http
        .send(get(mastodon, "/api/v1/accounts/verify_credentials"))
        .await
    {
        Ok(answer) if answer.succeeded() => answer,
        Ok(answer) => {
            return Audience::Unread(format!(
                "the server would not say whether the account approves its followers, as it answered {}",
                answer.status
            ));
        }
        Err(error) => {
            return Audience::Unread(format!(
                "the server could not be asked whether the account approves its followers: {error}"
            ));
        }
    };
    let said: Value = serde_json::from_slice(&answer.body).unwrap_or_default();
    match said.get("locked").and_then(Value::as_bool) {
        Some(true) => Audience::Private,
        Some(false) => Audience::Public(
            "the account takes followers without approval, so anyone may follow it and read its followers-only statuses; lock the account".to_owned(),
        ),
        None => Audience::Unread(
            "the server did not say whether the account approves its followers".to_owned(),
        ),
    }
}

//! The requests Forgejo's API takes, which its sources and stores share.

use herald_core::media;
use herald_ports::{Method, Request, Secret, SecretText};

/// Codeberg's API, the Forgejo most projects are on.
pub const API: &str = "https://codeberg.org/api/v1";

/// What Forgejo's answers are called in an error.
pub const FORGEJO: &str = "Forgejo";

/// `request` carrying `token` as Forgejo's API takes one.
#[must_use]
pub fn authorized(request: Request, token: Secret) -> Request {
    request.header(
        "Authorization",
        SecretText::plain("token ").then_secret(token),
    )
}

/// A request to Forgejo's API at `url`, asking for JSON, identified as
/// herald's, and carrying `token`.
#[must_use]
pub fn request(method: Method, url: String, token: Secret) -> Request {
    authorized(
        Request::new(method, SecretText::plain(url))
            .accepting(media::JSON)
            .identified(),
        token,
    )
}

#[cfg(test)]
mod tests {
    use herald_ports::USER_AGENT;

    use super::*;

    #[test]
    fn a_request_to_forgejo_carries_its_token_as_forgejo_takes_one() {
        let request = request(
            Method::Get,
            format!("{API}/repos/o/r"),
            Secret::new("TOKEN", "t0ken-value"),
        );
        assert_eq!(
            request.to_string(),
            format!(
                "GET https://codeberg.org/api/v1/repos/o/r\nAccept: application/json\nUser-Agent: {USER_AGENT}\nAuthorization: token «secret:TOKEN»"
            )
        );
    }
}

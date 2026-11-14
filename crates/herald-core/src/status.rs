//! The HTTP statuses herald answers with or reads, by name.

/// The request succeeded.
pub const OK: u16 = 200;
/// The request was taken, and its work is done after the answer.
pub const ACCEPTED: u16 = 202;
/// The first status past the successes: a redirection.
const REDIRECTION: u16 = 300;
/// What was asked for is unchanged since the copy the request names.
pub const NOT_MODIFIED: u16 = 304;
/// The request cannot be read.
pub const BAD_REQUEST: u16 = 400;
/// The request carries no credential the server takes.
pub const UNAUTHORIZED: u16 = 401;
/// The credential may not do what the request asks.
pub const FORBIDDEN: u16 = 403;
/// What the request names does not exist.
pub const NOT_FOUND: u16 = 404;
/// The method is not answered at the path.
pub const METHOD_NOT_ALLOWED: u16 = 405;
/// The request did not arrive in time.
pub const REQUEST_TIMEOUT: u16 = 408;
/// The request conflicts with what the server holds.
pub const CONFLICT: u16 = 409;
/// What the request names existed and is gone for good.
pub const GONE: u16 = 410;
/// The request's body is larger than the server reads.
pub const CONTENT_TOO_LARGE: u16 = 413;
/// The request reads, but the server cannot do what it asks.
pub const UNPROCESSABLE: u16 = 422;
/// The client sent too often.
pub const TOO_MANY_REQUESTS: u16 = 429;
/// The server failed; every status from here on is the server's own failure.
pub const SERVER_ERROR: u16 = 500;
/// The server cannot answer now.
pub const UNAVAILABLE: u16 = 503;

/// Whether `status` is a success.
#[must_use]
pub const fn succeeded(status: u16) -> bool {
    status >= OK && status < REDIRECTION
}

/// What to do about a refusal of `status`, where herald knows a remedy.
#[must_use]
pub const fn remedy(status: u16) -> Option<&'static str> {
    match status {
        UNAUTHORIZED => Some("check the credential herald sends, and that it has not expired"),
        FORBIDDEN => Some("check what herald's credential is allowed to do there"),
        NOT_FOUND => Some("check the names and ids the config gives"),
        TOO_MANY_REQUESTS => Some("herald sent more than the platform takes at once"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_success_is_a_status_of_the_200s() {
        assert!(!succeeded(199));
        assert!(succeeded(OK));
        assert!(succeeded(299));
        assert!(!succeeded(300));
    }

    #[test]
    fn each_refusal_a_config_can_cure_names_its_remedy() {
        assert_eq!(
            remedy(UNAUTHORIZED),
            Some("check the credential herald sends, and that it has not expired")
        );
        assert_eq!(
            remedy(FORBIDDEN),
            Some("check what herald's credential is allowed to do there")
        );
        assert_eq!(
            remedy(NOT_FOUND),
            Some("check the names and ids the config gives")
        );
        assert_eq!(
            remedy(TOO_MANY_REQUESTS),
            Some("herald sent more than the platform takes at once")
        );
        assert_eq!(remedy(CONFLICT), None);
        assert_eq!(remedy(SERVER_ERROR), None);
    }
}

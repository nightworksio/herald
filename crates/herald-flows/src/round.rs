//! The scheduled rounds a host with no timer of its own asks for over HTTP.

use herald_core::{bearer, same};
use herald_ports::Secret;

/// The variable naming the secret a request for a round carries, on a host
/// whose only way in is HTTP.
pub const ROUND_SECRET: &str = "HERALD_ROUND_SECRET";
/// What a round that ran is answered with.
pub const RAN: &str = "the round ran";

/// A scheduled round a host with no timer of its own asks for over HTTP:
/// `POST /herald/round/poll` or `/herald/round/housekeep`, carrying the secret
/// as a bearer token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Round {
    /// Every source polled.
    Poll,
    /// The store's housekeeping.
    Housekeep,
}

/// What a round asked for without its secret is refused with, kept for the
/// log.
#[must_use]
pub fn unasked() -> String {
    format!(
        "a round was asked without its secret; send it as Authorization: Bearer, and set {ROUND_SECRET} on the host where it is unset"
    )
}

impl Round {
    /// The round `path` names, where it names one.
    #[must_use]
    pub fn named(method: &str, path: &str) -> Option<Self> {
        match (method, path) {
            ("POST", "/herald/round/poll") => Some(Self::Poll),
            ("POST", "/herald/round/housekeep") => Some(Self::Housekeep),
            _ => None,
        }
    }

    /// Whether `authorization`, a request's header, holds `secret`. With no
    /// secret set no request is one, and the comparison reads every byte.
    #[must_use]
    pub fn allowed(authorization: Option<&str>, secret: Option<&Secret>) -> bool {
        authorization
            .and_then(bearer)
            .zip(secret)
            .is_some_and(|(given, secret)| same(secret.expose().as_bytes(), given.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_asked_without_its_secret_is_told_how_to_ask() {
        assert_eq!(
            unasked(),
            "a round was asked without its secret; send it as Authorization: Bearer, and set HERALD_ROUND_SECRET on the host where it is unset"
        );
    }

    #[test]
    fn a_round_is_named_by_its_path_and_only_on_a_post() {
        assert_eq!(
            Round::named("POST", "/herald/round/poll"),
            Some(Round::Poll)
        );
        assert_eq!(
            Round::named("POST", "/herald/round/housekeep"),
            Some(Round::Housekeep)
        );
        assert_eq!(Round::named("GET", "/herald/round/poll"), None);
        assert_eq!(Round::named("POST", "/herald/round/other"), None);
    }

    #[test]
    fn a_round_is_allowed_only_with_the_secret_that_is_set() {
        let secret = Secret::new("HERALD_ROUND_SECRET", "s3cret");
        assert!(Round::allowed(Some("Bearer s3cret"), Some(&secret)));
        assert!(!Round::allowed(Some("Bearer s3cres"), Some(&secret)));
        assert!(!Round::allowed(Some("Bearer s3cre"), Some(&secret)));
        let doubled = Secret::new("HERALD_ROUND_SECRET", "aa");
        assert!(!Round::allowed(Some("Bearer bb"), Some(&doubled)));
        assert!(!Round::allowed(Some("s3cret"), Some(&secret)));
        assert!(!Round::allowed(None, Some(&secret)));
        assert!(!Round::allowed(Some("Bearer s3cret"), None));
    }
}

//! The token a forge's sign-in hands an operator who applies a re-run: held
//! in memory for one command alone (ADR-0068).

use std::fmt;

use crate::secret::Secret;

/// What an operator's token is named as wherever it would be shown.
pub const OPERATOR_TOKEN: &str = "the operator's sign-in";

/// An operator's token from a forge's sign-in. Every way of showing it
/// names it alone; it implements no serialising, so no record, answer or
/// log line can hold it; it cannot be copied; and its value is overwritten
/// with zeroes when it is dropped.
pub struct OperatorToken(Secret);

impl OperatorToken {
    /// The token `value` the forge answered a sign-in with.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(Secret::new(OPERATOR_TOKEN, value))
    }

    /// The token as a request sends it, for the adapter that sends it.
    #[must_use]
    pub fn sent(&self) -> Secret {
        self.0.clone()
    }
}

impl fmt::Debug for OperatorToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.redacted())
    }
}

impl fmt::Display for OperatorToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.redacted())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Request};
    use crate::secret::SecretText;

    const VALUE: &str = "shown-nowhere";

    #[test]
    fn an_operator_s_token_is_shown_by_name_alone_and_sent_only_as_a_request_s_secret() {
        let token = OperatorToken::new(VALUE);
        let shown = [
            format!("{token}"),
            format!("{token:?}"),
            format!("{token:#?}"),
            format!("{:?}", Some(&token)),
            format!("{token:>40}"),
        ];
        for shown in shown {
            assert!(!shown.contains(VALUE), "{shown}");
            assert!(shown.contains(OPERATOR_TOKEN), "{shown}");
        }
        let request = Request::new(
            Method::Get,
            SecretText::plain("https://api.github.com/user"),
        )
        .bearer(token.sent());
        assert!(!format!("{request:?}").contains(VALUE));
        assert_eq!(token.sent().expose(), VALUE);
    }
}

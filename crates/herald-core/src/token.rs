//! A minted token kept in the store between runs, sealed (ADR-0041).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// A token kept sealed under a key only its minter holds, with when it
/// expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptToken {
    /// What the token is minted for, such as a GitHub App's installation.
    pub name: String,
    /// The token, sealed, in base64.
    pub sealed: String,
    /// When the token expires.
    pub expires: Timestamp,
}

impl KeptToken {
    /// The token `name`, sealed as `sealed`, expiring at `expires`.
    #[must_use]
    pub fn new(name: String, sealed: &[u8], expires: Timestamp) -> Self {
        Self {
            name,
            sealed: STANDARD.encode(sealed),
            expires,
        }
    }

    /// The sealed token, where its base64 reads.
    #[must_use]
    pub fn sealed_bytes(&self) -> Option<Vec<u8>> {
        STANDARD.decode(&self.sealed).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_token_holds_its_sealed_bytes_in_base64() {
        let kept = KeptToken::new("app".to_owned(), b"\x00sealed", Timestamp::UNIX_EPOCH);
        assert_eq!(kept.sealed, "AHNlYWxlZA==");
        assert_eq!(kept.sealed_bytes(), Some(b"\x00sealed".to_vec()));
        let unread = KeptToken {
            sealed: "not base64!".to_owned(),
            ..kept
        };
        assert_eq!(unread.sealed_bytes(), None);
    }
}

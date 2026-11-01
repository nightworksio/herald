use std::fmt;
use std::future::Future;

use herald_core::{Entry, Key, Marker};

use crate::crypto::{Algorithm, CryptoError, Signer};
use crate::secret::Secret;
use jiff::Timestamp;

/// Why a destination's posts could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkersError {
    /// The destination could not be read.
    Unreadable {
        /// What it said.
        reason: String,
    },
}

impl fmt::Display for MarkersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { reason } => {
                write!(f, "the destination's posts could not be read: {reason}")
            }
        }
    }
}

/// The marker of `message` under `key`: its HMAC-SHA256, as every post
/// carries it (ADR-0011).
///
/// # Errors
///
/// When `signer` cannot compute the MAC.
pub async fn marker<G: Signer>(
    signer: &G,
    key: &Secret,
    message: &[u8],
) -> Result<Marker, CryptoError> {
    signer
        .sign(Algorithm::HmacSha256, key, message)
        .await
        .map(|mac| Marker::of(&mac))
}

/// A thread herald opened from one of its own posts, as the destination
/// lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedThread {
    /// The destination's id of the thread.
    pub thread: String,
    /// When it was opened.
    pub opened: Timestamp,
    /// Whether it is archived.
    pub archived: bool,
    /// Whether it is locked.
    pub locked: bool,
}

/// The markers a destination's recent posts carry (ADR-0005, ADR-0011).
pub trait Markers: Sync {
    /// The ledger entry for `key` made by herald's own post carrying
    /// `marker`, the MAC of `key`: which steps landed, read from that post. A
    /// post by anyone else is never read as herald's.
    fn entry(
        &self,
        key: &Key,
        marker: &Marker,
    ) -> impl Future<Output = Result<Option<Entry>, MarkersError>> + Send;

    /// The threads opened from herald's own recent posts carrying the route
    /// marker `route`. A post by anyone else is never read as herald's.
    fn threads(
        &self,
        route: &Marker,
    ) -> impl Future<Output = Result<Vec<MarkedThread>, MarkersError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl Signer for Echo {
        fn sign(
            &self,
            algorithm: Algorithm,
            key: &Secret,
            message: &[u8],
        ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
            let signed = if key.expose() == "bad" {
                Err(CryptoError::BadKey {
                    name: key.name().to_owned(),
                })
            } else {
                Ok([format!("{algorithm:?}").as_bytes(), message].concat())
            };
            std::future::ready(signed)
        }
    }

    #[test]
    fn a_marker_is_the_hmac_of_its_message_under_the_key() {
        let key = Secret::new("K", "k");
        assert_eq!(
            pollster::block_on(marker(&Echo, &key, b"m")),
            Ok(Marker::of(b"HmacSha256m"))
        );
        let bad = Secret::new("K", "bad");
        assert!(pollster::block_on(marker(&Echo, &bad, b"m")).is_err());
    }

    #[test]
    fn the_error_reads_as_a_sentence() {
        assert_eq!(
            MarkersError::Unreadable {
                reason: "403".to_owned()
            }
            .to_string(),
            "the destination's posts could not be read: 403"
        );
    }
}

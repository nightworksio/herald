use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_ports::{Secret, StoreError};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// A record as a sealed store keeps it: its bytes as text, and their seal.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sealed {
    record: String,
    seal: String,
}

/// The key a store seals every record with (ADR-0005): an HMAC-SHA256 over
/// the record's path and bytes, so a record written by anyone without the
/// key, or moved to another path, reads as corrupt.
#[derive(Debug, Clone)]
pub struct Seal(Secret);

fn unsealed(path: &str) -> StoreError {
    StoreError::Corrupt {
        reason: format!("{path} is not sealed by this store's key"),
    }
}

impl Seal {
    /// Records sealed with `key`.
    #[must_use]
    pub const fn new(key: Secret) -> Self {
        Self(key)
    }

    fn mac(&self, path: &str, bytes: &[u8]) -> Option<Hmac<Sha256>> {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(self.0.expose().as_bytes()).ok()?;
        mac.update(path.as_bytes());
        mac.update(&[0]);
        mac.update(bytes);
        Some(mac)
    }

    /// `bytes`, the record at `path`, sealed.
    pub(crate) fn sealed(&self, path: &str, bytes: &[u8]) -> Vec<u8> {
        let seal = self
            .mac(path, bytes)
            .map(|mac| STANDARD.encode(mac.finalize().into_bytes()))
            .unwrap_or_default();
        let sealed = Sealed {
            record: String::from_utf8_lossy(bytes).into_owned(),
            seal,
        };
        serde_json::to_vec(&sealed).unwrap_or_default()
    }

    /// The record at `path` that `bytes` hold sealed.
    ///
    /// # Errors
    ///
    /// Where `bytes` hold no sealed record, or its seal is not this key's
    /// over `path` and the record.
    pub(crate) fn opened(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
        let sealed: Sealed = serde_json::from_slice(bytes).map_err(|_| unsealed(path))?;
        let seal = STANDARD.decode(&sealed.seal).map_err(|_| unsealed(path))?;
        let record = sealed.record.into_bytes();
        self.mac(path, &record)
            .filter(|mac| mac.clone().verify_slice(&seal).is_ok())
            .map(|_| record)
            .ok_or_else(|| unsealed(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seal(key: &str) -> Seal {
        Seal::new(Secret::new("HERALD_STORE_KEY", key))
    }

    #[test]
    fn a_sealed_record_opens_under_its_key_at_its_path_alone() {
        let sealed = seal("k").sealed("ledger/a.json", b"{\"version\":1}");
        assert_eq!(
            seal("k").opened("ledger/a.json", &sealed),
            Ok(b"{\"version\":1}".to_vec())
        );
        let refused = |path: &str| Err(unsealed(path));
        assert_eq!(
            seal("other").opened("ledger/a.json", &sealed),
            refused("ledger/a.json")
        );
        assert_eq!(
            seal("k").opened("ledger/b.json", &sealed),
            refused("ledger/b.json")
        );
        assert_eq!(
            seal("k").opened("ledger/a.json", b"{\"version\":1}"),
            refused("ledger/a.json")
        );
        let forged = String::from_utf8(sealed)
            .unwrap()
            .replace("{\\\"version\\\":1}", "{\\\"version\\\":2}");
        assert_eq!(
            seal("k").opened("ledger/a.json", forged.as_bytes()),
            refused("ledger/a.json")
        );
        let unreadable = br#"{"record":"{}","seal":"not base64!"}"#;
        assert_eq!(
            seal("k").opened("ledger/a.json", unreadable),
            refused("ledger/a.json")
        );
        assert_eq!(
            unsealed("ledger/a.json").to_string(),
            StoreError::Corrupt {
                reason: "ledger/a.json is not sealed by this store's key".to_owned()
            }
            .to_string()
        );
    }
}

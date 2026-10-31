use std::fmt;
use std::future::Future;

/// The version a key-value store keeps beside a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Versionstamp(pub String);

/// A value and its versionstamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamped {
    /// The value.
    pub value: Vec<u8>,
    /// Its versionstamp.
    pub versionstamp: Versionstamp,
}

/// A key and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// The key.
    pub key: Vec<String>,
    /// The value.
    pub value: Vec<u8>,
}

/// Why a key-value store could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvError {
    /// The store could not be reached.
    Unavailable {
        /// What it said.
        reason: String,
    },
}

impl fmt::Display for KvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { reason } => {
                write!(f, "the key-value store could not be reached: {reason}")
            }
        }
    }
}

/// A key-value store with versioned reads and an atomic check: Deno KV.
pub trait Kv: Sync {
    /// The value at `key`, and its versionstamp.
    fn get(&self, key: &[String]) -> impl Future<Output = Result<Option<Stamped>, KvError>> + Send;

    /// Sets `key` to `value` if its versionstamp is `expected`, or it is absent
    /// when `expected` is `None`. Answers the new versionstamp, or `None` when
    /// the check failed and nothing was written.
    fn set_if(
        &self,
        key: &[String],
        value: Vec<u8>,
        expected: Option<Versionstamp>,
    ) -> impl Future<Output = Result<Option<Versionstamp>, KvError>> + Send;

    /// Deletes `key`.
    fn delete(&self, key: &[String]) -> impl Future<Output = Result<(), KvError>> + Send;

    /// Every key under `prefix`, with its value.
    fn list(&self, prefix: &[String]) -> impl Future<Output = Result<Vec<Pair>, KvError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_reads_as_a_sentence() {
        assert_eq!(
            KvError::Unavailable {
                reason: "down".to_owned()
            }
            .to_string(),
            "the key-value store could not be reached: down"
        );
    }
}

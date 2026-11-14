use herald_core::Attempts;

use crate::Secret;

/// How a store keeps its records, beside its kind's own options: the key
/// it seals them with, where there is one (ADR-0005), and how many times it
/// tries a write that lost a race to an unrelated write.
#[derive(Debug, Clone, Default)]
pub struct RecordOptions {
    /// The key every record is sealed with.
    pub seal: Option<Secret>,
    /// How many times a write that lost a race is tried.
    pub attempts: Attempts,
}

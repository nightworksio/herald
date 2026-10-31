use jiff::Timestamp;

/// The current instant.
pub trait Clock: Sync {
    /// Now.
    fn now(&self) -> Timestamp;
}

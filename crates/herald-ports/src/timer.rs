use std::future::Future;
use std::time::Duration;

/// What waits for a while to pass: a heartbeat's interval, or a pause
/// before trying again.
pub trait Timer: Sync {
    /// Completes once `duration` has passed.
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

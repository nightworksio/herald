use std::sync::Mutex;
use std::time::Duration;

use herald_ports::Timer;

use crate::lock;

/// A timer that lets each wait pass at once, once whatever else is ready
/// has run, and keeps each wait it was asked for.
#[derive(Debug, Default)]
pub struct FakeTimer {
    waited: Mutex<Vec<Duration>>,
}

impl FakeTimer {
    /// Every wait asked for, in turn.
    #[must_use]
    pub fn waited(&self) -> Vec<Duration> {
        lock(&self.waited).clone()
    }
}

impl Timer for FakeTimer {
    async fn sleep(&self, duration: Duration) {
        lock(&self.waited).push(duration);
        let mut yielded = false;
        std::future::poll_fn(|context| {
            if yielded {
                return std::task::Poll::Ready(());
            }
            yielded = true;
            context.waker().wake_by_ref();
            std::task::Poll::Pending
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_wait_passes_and_is_kept() {
        let timer = FakeTimer::default();
        pollster::block_on(timer.sleep(Duration::from_secs(1)));
        pollster::block_on(timer.sleep(Duration::from_secs(2)));
        assert_eq!(
            timer.waited(),
            [Duration::from_secs(1), Duration::from_secs(2)]
        );
    }
}

use std::time::Duration;

use herald_ports::Timer;

/// The `Timer` of a native run, over Tokio's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioTimer;

impl Timer for TokioTimer {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_wait_passes_once_its_duration_has() {
        let started = tokio::time::Instant::now();
        TokioTimer.sleep(Duration::from_secs(30)).await;
        assert_eq!(started.elapsed(), Duration::from_secs(30));
    }
}

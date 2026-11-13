//! The signals that stop the process: Ctrl-C, and `SIGTERM` where there are
//! signals.

use std::future::Future;

/// When the process is asked to stop, listened for from this call on: Ctrl-C,
/// or `SIGTERM` as a container runtime sends it where there are signals.
pub fn stopped() -> impl Future<Output = ()> {
    #[cfg(unix)]
    let asked = {
        use tokio::signal::unix::{SignalKind, signal};
        signalled(
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        )
    };
    #[cfg(not(unix))]
    let asked = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    asked
}

/// When either signal arrives, or at once where either cannot be listened
/// for.
#[cfg(unix)]
pub(crate) async fn signalled(
    terminate: std::io::Result<tokio::signal::unix::Signal>,
    interrupt: std::io::Result<tokio::signal::unix::Signal>,
) {
    if let (Ok(mut terminate), Ok(mut interrupt)) = (terminate, interrupt) {
        futures_util::future::select(
            std::pin::pin!(terminate.recv()),
            std::pin::pin!(interrupt.recv()),
        )
        .await;
    }
}

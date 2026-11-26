//! The signals that stop the process: Ctrl-C, and `SIGTERM` where there are
//! signals.

use std::future::Future;

/// Why a server stops at once where `signal` cannot be listened for, as
/// `error` says.
fn unheard(signal: &str, error: &std::io::Error) -> String {
    format!("herald cannot listen for {signal}, so the server stops at once: {error}")
}

/// When the process is asked to stop, listened for from this call on: Ctrl-C,
/// or `SIGTERM` as a container runtime sends it where there are signals.
///
/// # Errors
///
/// At once, where a signal cannot be listened for, saying why.
pub fn stopped() -> impl Future<Output = Result<(), String>> {
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
        tokio::signal::ctrl_c()
            .await
            .map_err(|error| unheard("Ctrl-C", &error))
    };
    asked
}

/// When either signal arrives.
///
/// # Errors
///
/// At once, where either cannot be listened for, saying why.
#[cfg(unix)]
pub(crate) async fn signalled(
    terminate: std::io::Result<tokio::signal::unix::Signal>,
    interrupt: std::io::Result<tokio::signal::unix::Signal>,
) -> Result<(), String> {
    let mut terminate = terminate.map_err(|error| unheard("SIGTERM", &error))?;
    let mut interrupt = interrupt.map_err(|error| unheard("SIGINT", &error))?;
    futures_util::future::select(
        std::pin::pin!(terminate.recv()),
        std::pin::pin!(interrupt.recv()),
    )
    .await;
    Ok(())
}

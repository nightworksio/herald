//! The `herald` command.

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let vars = std::env::vars().collect();
    let code = herald::run_until(
        &args,
        &vars,
        &herald::native,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
        stopped(),
    )
    .await;
    ExitCode::from(code)
}

/// When the process is asked to stop, listened for from this call on: Ctrl-C,
/// or `SIGTERM` as a container runtime sends it.
#[cfg(unix)]
fn stopped() -> impl std::future::Future<Output = ()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate());
    let mut interrupt = signal(SignalKind::interrupt());
    async move {
        if let (Ok(terminate), Ok(interrupt)) = (&mut terminate, &mut interrupt) {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = interrupt.recv() => {}
            }
        }
    }
}

/// When the process is asked to stop: Ctrl-C.
#[cfg(not(unix))]
async fn stopped() {
    let _ = tokio::signal::ctrl_c().await;
}

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
/// or `SIGTERM` as a container runtime sends it where there are signals.
fn stopped() -> impl std::future::Future<Output = ()> {
    #[cfg(unix)]
    let asked = {
        use std::pin::pin;

        use futures_util::future::select;
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate());
        let mut interrupt = signal(SignalKind::interrupt());
        async move {
            if let (Ok(terminate), Ok(interrupt)) = (&mut terminate, &mut interrupt) {
                select(pin!(terminate.recv()), pin!(interrupt.recv())).await;
            }
        }
    };
    #[cfg(not(unix))]
    let asked = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    asked
}

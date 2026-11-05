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

/// When the process is asked to stop: Ctrl-C, or `SIGTERM` as a container
/// runtime sends it.
async fn stopped() {
    #[cfg(unix)]
    let terminated = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .ok()?
            .recv()
            .await
    };
    #[cfg(not(unix))]
    let terminated = std::future::pending::<Option<()>>();
    tokio::select! {
        Ok(()) = tokio::signal::ctrl_c() => {}
        Some(()) = terminated => {}
    }
}

//! The `herald` command.

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let vars = std::env::vars().collect();
    let code = herald::run(
        &args,
        &vars,
        &herald::native,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
    .await;
    ExitCode::from(code)
}

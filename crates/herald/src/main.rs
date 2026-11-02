//! The `herald` command.

use std::process::ExitCode;

use herald::Connection;
use herald_native::NativeHttp;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let vars = std::env::vars().collect();
    let connect = |base: &str| {
        NativeHttp::reaching(base)
            .map(Connection::new)
            .map_err(|error| error.to_string())
    };
    let code = herald::run(
        &args,
        &vars,
        &connect,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
    .await;
    ExitCode::from(code)
}

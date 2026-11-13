//! A request answering a command, sent to the destination the command was
//! given on (ADR-0016).

use herald_kinds::Interacted;
use herald_ports::{Http, Request};

/// Whether `interacting` took `request`, sent through `http`, answering a
/// command, or else why not.
///
/// # Errors
///
/// What the destination said.
pub(crate) async fn sent<H: Http>(
    interacting: &Interacted,
    http: &H,
    request: Request,
) -> Result<(), String> {
    let response = http
        .send(request)
        .await
        .map_err(|error| error.to_string())?;
    interacting.accepted(&response)
}

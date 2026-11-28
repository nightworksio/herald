//! A request answering a command, made ready and sent to the destination
//! the command was given on (ADR-0016).

use herald_kinds::Interacted;
use herald_ports::{Http, Request};
use jiff::Timestamp;

/// Whether `interacting` took `request`, made ready at `now` and sent
/// through `http`, answering a command, or else why not.
///
/// # Errors
///
/// Why it could not be made ready, or what the destination said.
pub(crate) async fn sent<H: Http>(
    interacting: &Interacted,
    http: &H,
    request: Request,
    now: Timestamp,
) -> Result<(), String> {
    let request = interacting.authorised(http, request, now).await?;
    let response = http
        .send(request)
        .await
        .map_err(|error| error.to_string())?;
    interacting.accepted(&response)
}

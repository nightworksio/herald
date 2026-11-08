use wasip2::exports::http::incoming_handler::{IncomingRequest, ResponseOutparam};
use wasip2::http::types::{Fields, IncomingBody, Method, OutgoingBody, OutgoingResponse};
use wasip2::io::streams::StreamError;

use herald_compose::{Asked, told};
use herald_flows::{Reply, refusal};

use super::served::{Started, said};

const CHUNK: u64 = 64 * 1024;

fn method_of(method: &Method) -> String {
    match method {
        Method::Get => "GET".to_owned(),
        Method::Head => "HEAD".to_owned(),
        Method::Post => "POST".to_owned(),
        Method::Put => "PUT".to_owned(),
        Method::Delete => "DELETE".to_owned(),
        Method::Connect => "CONNECT".to_owned(),
        Method::Options => "OPTIONS".to_owned(),
        Method::Trace => "TRACE".to_owned(),
        Method::Patch => "PATCH".to_owned(),
        Method::Other(other) => other.clone(),
    }
}

/// The body of `incoming`, read no further than one byte past `most`, or
/// `None` where it cannot be read.
fn read(incoming: IncomingBody, most: usize) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    {
        let stream = incoming.stream().ok()?;
        loop {
            match stream.blocking_read(CHUNK) {
                Ok(chunk) => {
                    body.extend_from_slice(&chunk);
                    if body.len() > most {
                        return Some(body);
                    }
                }
                Err(StreamError::Closed) => break,
                Err(StreamError::LastOperationFailed(_)) => return None,
            }
        }
    }
    drop(IncomingBody::finish(incoming));
    Some(body)
}

fn respond(out: ResponseOutparam, reply: &Reply) {
    let body = &reply.body;
    let headers = Fields::new();
    let _ = headers.set("content-type", &[reply.kind.as_bytes().to_vec()]);
    let _ = headers.set("content-length", &[body.len().to_string().into_bytes()]);
    if let Some(after) = reply.retry_after {
        let _ = headers.set("retry-after", &[after.to_string().into_bytes()]);
    }
    let response = OutgoingResponse::new(headers);
    let _ = response.set_status_code(reply.status);
    let Ok(outgoing) = response.body() else {
        return;
    };
    ResponseOutparam::set(out, Ok(response));
    if let Ok(stream) = outgoing.write() {
        for chunk in body.chunks(4096) {
            if stream.blocking_write_and_flush(chunk).is_err() {
                break;
            }
        }
    }
    let _ = OutgoingBody::finish(outgoing, None);
}

/// Answers `request`, then does the work it leaves.
pub(super) async fn handled(request: IncomingRequest, out: ResponseOutparam) {
    let method = method_of(&request.method());
    let target = request.path_with_query().unwrap_or_else(|| "/".to_owned());
    #[cfg(feature = "conformance")]
    if let Some(reply) = super::conformance::answered(&target).await {
        respond(out, &reply);
        return;
    }
    let mut headers: Vec<(String, String)> = request
        .headers()
        .entries()
        .into_iter()
        .filter_map(|(name, value)| Some((name, String::from_utf8(value).ok()?)))
        .collect();
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"))
        && let Some(authority) = request.authority()
    {
        headers.push(("host".to_owned(), authority));
    }
    let started = match Started::new(std::env::vars().collect()).await {
        Ok(started) => started,
        Err(reply) => {
            respond(out, &reply);
            return;
        }
    };
    let most = started.body();
    let Some(body) = request
        .consume()
        .ok()
        .and_then(|incoming| read(incoming, most))
    else {
        let refused = refusal(400, "its body could not be read");
        said(refused.refused.as_deref().map(told::refused));
        respond(out, &refused.reply);
        return;
    };
    let asked = Asked {
        method,
        target,
        headers,
        body,
    };
    let (reply, left) = started.answered(&asked).await;
    respond(out, &reply);
    if let Some(left) = left {
        started.worked(left).await;
    }
}

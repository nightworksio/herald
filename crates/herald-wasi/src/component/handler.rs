use wasip2::exports::http::incoming_handler::{IncomingRequest, ResponseOutparam};
use wasip2::http::types::{Fields, IncomingBody, Method, OutgoingBody, OutgoingResponse};
use wasip2::io::streams::StreamError;

use herald_compose::Asked;

use super::served::{answered, worked};

/// The most of a request's body that is read.
const MOST: usize = 1 << 20;
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

/// The body of `incoming`, or `None` where it is larger than [`MOST`].
fn read(incoming: IncomingBody) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    {
        let stream = incoming.stream().ok()?;
        loop {
            match stream.blocking_read(CHUNK) {
                Ok(chunk) => {
                    body.extend_from_slice(&chunk);
                    if body.len() > MOST {
                        return None;
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

fn respond(out: ResponseOutparam, status: u16, kind: &str, body: &[u8]) {
    let headers = Fields::new();
    let _ = headers.set("content-type", &[kind.as_bytes().to_vec()]);
    let _ = headers.set("content-length", &[body.len().to_string().into_bytes()]);
    let response = OutgoingResponse::new(headers);
    let _ = response.set_status_code(status);
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
    let headers = request
        .headers()
        .entries()
        .into_iter()
        .filter_map(|(name, value)| Some((name, String::from_utf8(value).ok()?)))
        .collect();
    let Some(body) = request.consume().ok().and_then(read) else {
        respond(
            out,
            413,
            "text/plain; charset=utf-8",
            b"the body is too large",
        );
        return;
    };
    let asked = Asked {
        method,
        target,
        headers,
        body,
    };
    let (reply, left) = answered(&asked, std::env::vars().collect()).await;
    respond(out, reply.status, reply.kind, &reply.body);
    if let Some((composed, job)) = left {
        worked(&composed, job).await;
    }
}

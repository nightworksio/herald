use std::future::Future;
use std::time::Duration;

use herald_compose::{Reaching, received};
use herald_ports::{Http, HttpError, Request, Response};
use http_body_util::BodyExt as _;
use send_wrapper::SendWrapper;
use wstd::future::FutureExt as _;
use wstd::http::{Body, Client, Method};

/// An `Http` over `wasi:http`, reaching what one connection may. The component
/// host's own policy decides what else it may reach; an address the component
/// cannot resolve is not vetted here, as on a Worker.
#[derive(Debug, Clone)]
pub(super) struct WasiHttp {
    reaching: Reaching,
    timeout: Duration,
}

impl WasiHttp {
    pub(super) fn new(reaching: Reaching, timeout: Duration) -> Self {
        Self { reaching, timeout }
    }
}

fn unreachable(reason: &impl ToString) -> HttpError {
    HttpError::Unreachable {
        reason: reason.to_string(),
    }
}

/// `body` read frame by frame to at most one byte past `most`, which
/// `received` then refuses, so a larger answer is never held whole.
async fn read(body: Body, most: usize) -> Result<Vec<u8>, HttpError> {
    let mut stream = body.into_boxed_body();
    let mut bytes = Vec::new();
    while bytes.len() <= most {
        let Some(frame) = stream.frame().await else {
            break;
        };
        if let Ok(data) = frame.map_err(|error| unreachable(&error))?.into_data() {
            let room = most.saturating_add(1).saturating_sub(bytes.len());
            bytes.extend_from_slice(data.get(..room.min(data.len())).unwrap_or_default());
        }
    }
    Ok(bytes)
}

fn method(request: &Request) -> Result<Method, HttpError> {
    Method::from_bytes(request.method.as_str().as_bytes()).map_err(|error| unreachable(&error))
}

impl Http for WasiHttp {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        let reaching = self.reaching.clone();
        let timeout = self.timeout;
        let exchange = async move {
            let url = reaching.admitted(&request.url)?;
            let mut asked = wstd::http::Request::builder()
                .method(method(&request)?)
                .uri(url.as_str());
            for (name, value) in &request.headers {
                asked = asked.header(name.as_str(), value.expose());
            }
            let asked = asked
                .body(wstd::http::Body::from(request.body.clone()))
                .map_err(|error| unreachable(&error))?;
            let mut client = Client::new();
            client.set_connect_timeout(timeout);
            client.set_first_byte_timeout(timeout);
            client.set_between_bytes_timeout(timeout);
            let (head, body) = client
                .send(asked)
                .await
                .map_err(|error| unreachable(&error))?
                .into_parts();
            let bytes = read(body, request.most).await?;
            let headers = head
                .headers
                .iter()
                .filter_map(|(name, value)| {
                    Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
                })
                .collect();
            received(
                &request,
                Response {
                    status: head.status.as_u16(),
                    headers,
                    body: bytes,
                },
            )
        };
        SendWrapper::new(async move {
            exchange
                .timeout(wstd::time::Duration::from(timeout))
                .await
                .map_err(|_| HttpError::TimedOut)?
        })
    }
}

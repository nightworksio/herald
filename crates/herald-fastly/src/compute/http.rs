use std::future::{Future, ready};
use std::io::Read as _;
use std::time::Duration;

use fastly::Backend;
use fastly::http::Method;
use herald_compose::{Reaching, received};
use herald_ports::{Http, HttpError, Outbound, Request, Response};

/// An `Http` over a dynamic backend per origin, reaching what one connection
/// may. The service must have dynamic backends enabled.
#[derive(Debug, Clone)]
pub(super) struct FastlyHttp {
    reaching: Reaching,
    connect: Duration,
    total: Duration,
    body: usize,
}

/// The dynamic backend's name for an origin: one name per scheme, host and
/// port, and never the same name for two of them.
pub(super) fn backend_name(scheme: &str, host: &str, port: u16) -> String {
    format!(
        "h_{}",
        herald_core::digest(&format!("{scheme}://{host}:{port}"))
    )
}

impl FastlyHttp {
    pub(super) fn new(reaching: Reaching, outbound: &Outbound) -> Self {
        Self {
            reaching,
            connect: outbound.connect.unsigned_abs(),
            total: outbound.total.unsigned_abs(),
            body: outbound.body,
        }
    }

    fn sent(&self, request: &Request) -> Result<Response, HttpError> {
        let url = self.reaching.admitted(&request.url)?;
        let host = url.host_str().unwrap_or_default();
        let port = url.port_or_known_default().unwrap_or(443);
        let secure = url.scheme() == "https";
        let name = backend_name(url.scheme(), host, port);
        let backend = Backend::builder(&name, format!("{host}:{port}"))
            .override_host(host)
            .connect_timeout(self.connect)
            .first_byte_timeout(self.total)
            .between_bytes_timeout(self.total);
        let backend = if secure {
            backend.enable_ssl().sni_hostname(host)
        } else {
            backend
        };
        let backend = match backend.finish() {
            Ok(backend) => backend,
            Err(fastly::backend::BackendCreationError::NameInUse) => {
                let kept = Backend::from_name(&name).map_err(HttpError::unreachable)?;
                if kept.get_host() != host || kept.get_port() != port || kept.is_ssl() != secure {
                    return Err(HttpError::unreachable(format_args!(
                        "the backend {name} reaches another origin than {host}:{port}"
                    )));
                }
                kept
            }
            Err(error) => return Err(HttpError::unreachable(error)),
        };
        let method = Method::from_bytes(request.method.as_str().as_bytes())
            .map_err(HttpError::unreachable)?;
        let mut asked = fastly::Request::new(method, url.as_str());
        for (name, value) in &request.headers {
            asked.append_header(name.as_str(), value.expose());
        }
        asked.set_body(request.body.clone());
        let mut answer = asked
            .send(backend)
            .map_err(|error| HttpError::unreachable(error.root_cause()))?;
        let status = answer.get_status().as_u16();
        let headers = answer
            .get_headers()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        let limit = u64::try_from(request.most)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut body = Vec::new();
        answer
            .take_body()
            .take(limit)
            .read_to_end(&mut body)
            .map_err(HttpError::unreachable)?;
        received(
            request,
            Response {
                status,
                headers,
                body,
            },
        )
    }
}

impl Http for FastlyHttp {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        ready(self.sent(&request.within(self.body)))
    }
}

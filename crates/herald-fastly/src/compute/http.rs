use std::future::{Future, ready};
use std::io::Read as _;
use std::time::Duration;

use fastly::Backend;
use fastly::http::Method;
use herald_compose::{Reaching, received};
use herald_ports::{Http, HttpError, Request, Response};

const TIMEOUT: Duration = Duration::from_secs(10);

/// An `Http` over a dynamic backend per origin, reaching what one connection
/// may. The service must have dynamic backends enabled.
#[derive(Debug, Clone)]
pub(super) struct FastlyHttp {
    reaching: Reaching,
}

impl FastlyHttp {
    pub(super) fn new(reaching: Reaching) -> Self {
        Self { reaching }
    }

    fn sent(&self, request: &Request) -> Result<Response, HttpError> {
        let unreachable = |reason: String| HttpError::Unreachable { reason };
        let url = self.reaching.admitted(&request.url)?;
        let host = url.host_str().unwrap_or_default();
        let port = url.port_or_known_default().unwrap_or(443);
        let name: String = format!("h_{host}_{port}")
            .chars()
            .map(|one| {
                if one.is_ascii_alphanumeric() {
                    one
                } else {
                    '_'
                }
            })
            .collect();
        let backend = Backend::builder(&name, format!("{host}:{port}"))
            .override_host(host)
            .connect_timeout(TIMEOUT)
            .first_byte_timeout(TIMEOUT)
            .between_bytes_timeout(TIMEOUT);
        let backend = if url.scheme() == "https" {
            backend.enable_ssl().sni_hostname(host)
        } else {
            backend
        };
        let backend = match backend.finish() {
            Ok(backend) => backend,
            Err(fastly::backend::BackendCreationError::NameInUse) => {
                Backend::from_name(&name).map_err(|error| unreachable(error.to_string()))?
            }
            Err(error) => return Err(unreachable(error.to_string())),
        };
        let method = Method::from_bytes(request.method.as_str().as_bytes())
            .map_err(|error| unreachable(error.to_string()))?;
        let mut asked = fastly::Request::new(method, url.as_str());
        for (name, value) in &request.headers {
            asked.append_header(name.as_str(), value.expose());
        }
        asked.set_body(request.body.clone());
        let mut answer = asked
            .send(backend)
            .map_err(|error| unreachable(error.root_cause().to_string()))?;
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
            .map_err(|error| unreachable(error.to_string()))?;
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
        ready(self.sent(&request))
    }
}

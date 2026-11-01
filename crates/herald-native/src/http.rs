use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use herald_ports::{Http, HttpError, Method, Request, Response};
use rustls::crypto::CryptoProvider;
use rustls::{ClientConfig, RootCertStore};
use url::Url;

use crate::reach::Reach;

/// How long a connection may take to open.
const CONNECT: Duration = Duration::from_secs(10);
/// How long a request may take, from sending it to the end of its response.
const TOTAL: Duration = Duration::from_secs(30);
/// The largest response body read, in bytes.
const BODY_CAP: usize = 8_388_608;

/// Why the HTTP client could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError(String);

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the HTTP client could not be built: {}", self.0)
    }
}

/// `Http` over `reqwest` with rustls and the Mozilla roots, reaching only the
/// origins of its [`Reach`] and following no redirect (ADR-0007).
#[derive(Debug, Clone)]
pub struct NativeHttp {
    client: reqwest::Client,
    reach: Reach,
}

impl NativeHttp {
    /// A client reaching only `reach`.
    ///
    /// # Errors
    ///
    /// When TLS or the client cannot be set up.
    pub fn new(reach: Reach) -> Result<Self, BuildError> {
        let tls = tls(rustls::crypto::ring::default_provider())?;
        Self::built(reach, Some(tls), CONNECT, TOTAL)
    }

    pub(crate) fn built(
        reach: Reach,
        tls: Option<ClientConfig>,
        connect: Duration,
        total: Duration,
    ) -> Result<Self, BuildError> {
        let builder = reqwest::Client::builder();
        let builder = match tls {
            Some(tls) => builder.tls_backend_preconfigured(tls),
            None => builder.tls_backend_preconfigured(()),
        };
        let client = builder
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(connect)
            .timeout(total)
            .build()
            .map_err(|error| BuildError(chain(&error)))?;
        Ok(Self { client, reach })
    }
}

fn tls(provider: CryptoProvider) -> Result<ClientConfig, BuildError> {
    let roots: RootCertStore = webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect();
    let config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|error| BuildError(error.to_string()))?;
    Ok(config.with_root_certificates(roots).with_no_client_auth())
}

fn method(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
    }
}

fn chain(error: &dyn Error) -> String {
    let mut reason = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        reason = format!("{reason}: {cause}");
        source = cause.source();
    }
    reason
}

fn failed(error: reqwest::Error) -> HttpError {
    if error.is_timeout() {
        HttpError::TimedOut
    } else {
        HttpError::Unreachable {
            reason: chain(&error.without_url()),
        }
    }
}

async fn read(mut response: reqwest::Response) -> Result<Response, HttpError> {
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
            (name.as_str().to_owned(), value)
        })
        .collect();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        if body.len() + chunk.len() > BODY_CAP {
            return Err(HttpError::Unreachable {
                reason: format!("the response is larger than {BODY_CAP} bytes"),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Response {
        status,
        headers,
        body,
    })
}

impl Http for NativeHttp {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let reachable = Url::parse(&request.url.expose())
            .ok()
            .filter(|url| self.reach.allows(url));
        let Some(url) = reachable else {
            return Err(HttpError::Refused {
                url: request.url.redacted(),
            });
        };
        let mut builder = self.client.request(method(request.method), url);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.expose());
        }
        let response = builder.body(request.body).send().await.map_err(failed)?;
        read(response).await
    }
}

#[cfg(test)]
mod tests;

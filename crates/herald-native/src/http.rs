use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use herald_ports::{Http, HttpError, Method, Outbound, Proxy, Request, Response, Roots};
use jiff::SignedDuration;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject as _;
use rustls::{ClientConfig, RootCertStore};
use url::Url;

use herald_compose::{Reach, ReachError};

/// How long a connection may take to open where no config says.
pub(crate) const CONNECT: Duration = duration(Outbound::CONNECT);

/// `span` as a standard duration, its sign dropped.
pub(crate) const fn duration(span: SignedDuration) -> Duration {
    span.unsigned_abs()
}

/// Why the HTTP client could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError(pub(crate) String);

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the HTTP client could not be built: {}", self.0)
    }
}

/// Why an `Http` for a base URL could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// The base URL may not be reached.
    Reach(ReachError),
    /// The client could not be built.
    Build(BuildError),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reach(error) => error.fmt(f),
            Self::Build(error) => error.fmt(f),
        }
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
    /// A client reaching only `reach`, within `outbound`'s bounds, through
    /// its proxy and trusting its roots beside the Mozilla roots.
    ///
    /// # Errors
    ///
    /// When TLS, the proxy or the client cannot be set up.
    pub fn new(reach: Reach, outbound: &Outbound) -> Result<Self, BuildError> {
        let tls = trusting(
            rustls::crypto::ring::default_provider(),
            outbound.roots.as_ref(),
        )?;
        let builder = proxied(reqwest::Client::builder(), &outbound.proxy)?;
        let client = client(
            builder,
            Some(tls),
            duration(outbound.connect),
            Some(duration(outbound.total)),
        )?;
        Ok(Self { client, reach })
    }

    /// A client reaching only the origin of `base`, within `outbound`.
    ///
    /// # Errors
    ///
    /// When `base` may not be reached, or the client cannot be built.
    pub fn reaching(base: &str, outbound: &Outbound) -> Result<Self, ConnectError> {
        let reach = Reach::new([base]).map_err(ConnectError::Reach)?;
        Self::new(reach, outbound).map_err(ConnectError::Build)
    }

    /// A client reaching only the origin of `base`, with no bound on how
    /// long an answer takes: for an API that holds a request open until it
    /// has something to answer, as Lambda's runtime API does.
    ///
    /// # Errors
    ///
    /// When `base` may not be reached, or the client cannot be built.
    pub fn waiting(base: &str) -> Result<Self, ConnectError> {
        let reach = Reach::new([base]).map_err(ConnectError::Reach)?;
        let tls = tls(rustls::crypto::ring::default_provider()).map_err(ConnectError::Build)?;
        Self::built(reach, Some(tls), CONNECT, None).map_err(ConnectError::Build)
    }

    pub(crate) fn built(
        reach: Reach,
        tls: Option<ClientConfig>,
        connect: Duration,
        total: Option<Duration>,
    ) -> Result<Self, BuildError> {
        let client = client(reqwest::Client::builder(), tls, connect, total)?;
        Ok(Self { client, reach })
    }
}

/// A client from `builder` with `tls`, its timeouts, and no redirect
/// followed (ADR-0007).
pub(crate) fn client(
    builder: reqwest::ClientBuilder,
    tls: Option<ClientConfig>,
    connect: Duration,
    total: Option<Duration>,
) -> Result<reqwest::Client, BuildError> {
    let builder = match tls {
        Some(tls) => builder.tls_backend_preconfigured(tls),
        None => builder.tls_backend_preconfigured(()),
    };
    let builder = match total {
        Some(total) => builder.timeout(total),
        None => builder,
    };
    builder
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(connect)
        .build()
        .map_err(|error| BuildError(chain(&error)))
}

/// `builder` going through `proxy`.
fn proxied(
    builder: reqwest::ClientBuilder,
    proxy: &Proxy,
) -> Result<reqwest::ClientBuilder, BuildError> {
    match proxy {
        Proxy::Environment => Ok(builder),
        Proxy::None => Ok(builder.no_proxy()),
        Proxy::At(url) => reqwest::Proxy::all(url)
            .map(|proxy| builder.proxy(proxy))
            .map_err(|error| BuildError(chain(&error))),
    }
}

pub(crate) fn tls(provider: CryptoProvider) -> Result<ClientConfig, BuildError> {
    trusting(provider, None)
}

/// TLS trusting the Mozilla roots, and `roots` beside them.
pub(crate) fn trusting(
    provider: CryptoProvider,
    roots: Option<&Roots>,
) -> Result<ClientConfig, BuildError> {
    let trusted = trusted(roots)?;
    let config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|error| BuildError(error.to_string()))?;
    Ok(config.with_root_certificates(trusted).with_no_client_auth())
}

/// The Mozilla roots, and `roots` beside them.
pub(crate) fn trusted(roots: Option<&Roots>) -> Result<RootCertStore, BuildError> {
    let mut trusted: RootCertStore = webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect();
    if let Some(roots) = roots {
        let unread = |error: &dyn fmt::Display| BuildError(format!("{}: {error}", roots.file));
        for certificate in CertificateDer::pem_slice_iter(roots.pem.as_bytes()) {
            let certificate = certificate.map_err(|error| unread(&error))?;
            trusted.add(certificate).map_err(|error| unread(&error))?;
        }
    }
    Ok(trusted)
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

pub(crate) fn chain(error: &dyn Error) -> String {
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

async fn read(mut response: reqwest::Response, most: usize) -> Result<Response, HttpError> {
    let larger = || HttpError::Unreachable {
        reason: format!("the response is larger than {most} bytes"),
    };
    if response
        .content_length()
        .is_some_and(|length| usize::try_from(length).map_or(true, |length| length > most))
    {
        return Err(larger());
    }
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
        if body.len() + chunk.len() > most {
            return Err(larger());
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
        sent(&self.client, request, url).await
    }
}

/// `request` sent to `url` through `client`, and its response read.
pub(crate) async fn sent(
    client: &reqwest::Client,
    request: Request,
    url: Url,
) -> Result<Response, HttpError> {
    let mut builder = client.request(method(request.method), url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.expose());
    }
    let most = request.most;
    let response = builder.body(request.body).send().await.map_err(failed)?;
    read(response, most).await
}

#[cfg(test)]
mod tests;

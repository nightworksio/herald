use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use herald_compose::{public, reachable};
use herald_ports::{Http, HttpError, Request, Response};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use rustls::ClientConfig;
use url::Url;

use crate::http::{BuildError, client, sent, tls};

/// How long a connection to a host reached on others' behalf may take to
/// open.
const CONNECT: Duration = Duration::from_secs(5);
/// How long a request to such a host may take, from sending it to the end
/// of its response.
const TOTAL: Duration = Duration::from_secs(10);

/// `Http` for the hosts herald reaches on others' behalf, an `ActivityPub`
/// actor's key and a follower's inbox: any HTTPS origin, connected to only
/// where every address its host resolves to is public, through no proxy and
/// following no redirect (ADR-0007).
#[derive(Debug, Clone)]
pub struct PublicHttp {
    client: reqwest::Client,
    admits: fn(IpAddr) -> bool,
    scheme: &'static str,
}

/// A host that resolves to an address no request may reach.
#[derive(Debug)]
struct NotPublic(String);

impl fmt::Display for NotPublic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} resolves to an address that is not public", self.0)
    }
}

impl std::error::Error for NotPublic {}

/// A resolver answering a host only with its addresses, and only where
/// every one of them is admitted, so the client connects to the address it
/// checked.
#[derive(Debug)]
struct Guard(fn(IpAddr) -> bool);

impl Resolve for Guard {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let admits = self.0;
        Box::pin(async move {
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            vetted(host, found, admits)
        })
    }
}

fn vetted(
    host: String,
    found: Vec<SocketAddr>,
    admits: fn(IpAddr) -> bool,
) -> Result<Addrs, Box<dyn std::error::Error + Send + Sync>> {
    if !admitted(&found, admits) {
        return Err(Box::new(NotPublic(host)));
    }
    Ok(Box::new(found.into_iter()))
}

/// Whether a host resolving to `found` may be reached: it resolves to at
/// least one address, and `admits` every one.
fn admitted(found: &[SocketAddr], admits: fn(IpAddr) -> bool) -> bool {
    !found.is_empty() && found.iter().all(|address| admits(address.ip()))
}

impl PublicHttp {
    /// A client reaching public HTTPS origins.
    ///
    /// # Errors
    ///
    /// When TLS or the client cannot be set up.
    pub fn new() -> Result<Self, BuildError> {
        let tls = tls(rustls::crypto::ring::default_provider())?;
        Self::built(public, "https", Some(tls), CONNECT, Some(TOTAL))
    }

    pub(crate) fn built(
        admits: fn(IpAddr) -> bool,
        scheme: &'static str,
        tls: Option<ClientConfig>,
        connect: Duration,
        total: Option<Duration>,
    ) -> Result<Self, BuildError> {
        let builder = reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(Guard(admits));
        Ok(Self {
            client: client(builder, tls, connect, total)?,
            admits,
            scheme,
        })
    }

    fn reachable(&self, url: &Url) -> bool {
        reachable(url, self.scheme, self.admits)
    }
}

impl Http for PublicHttp {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let reachable = Url::parse(&request.url.expose())
            .ok()
            .filter(|url| self.reachable(url));
        let Some(url) = reachable else {
            return Err(HttpError::Refused {
                url: request.url.redacted(),
            });
        };
        sent(&self.client, request, url).await
    }
}

#[cfg(test)]
mod tests;

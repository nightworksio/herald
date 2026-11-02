use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use herald_ports::{Http, HttpError, Request, Response};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use rustls::ClientConfig;
use url::{Host, Url};

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
        Self::built(public, "https", Some(tls), CONNECT, TOTAL)
    }

    pub(crate) fn built(
        admits: fn(IpAddr) -> bool,
        scheme: &'static str,
        tls: Option<ClientConfig>,
        connect: Duration,
        total: Duration,
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
        url.scheme() == self.scheme
            && url.username().is_empty()
            && url.password().is_none()
            && match url.host() {
                Some(Host::Domain(_)) => true,
                Some(Host::Ipv4(address)) => (self.admits)(IpAddr::V4(address)),
                Some(Host::Ipv6(address)) => (self.admits)(IpAddr::V6(address)),
                None => false,
            }
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

/// Whether `address` is on the public internet. An IPv4 address is not
/// loopback, private, shared, link-local, documentation, benchmarking,
/// reserved, multicast or unspecified. An IPv6 address is a global unicast
/// one, in `2000::/3`, outside its IETF protocol assignments and
/// documentation ranges, or carries such an IPv4 address.
#[must_use]
pub fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => public_v6(address),
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_documentation()
        || address.is_multicast()
        || first == 0
        || first >= 240
        || (first == 100 && (64..=127).contains(&second))
        || (first == 192 && second == 0 && third == 0)
        || (first == 198 && (second == 18 || second == 19)))
}

fn public_v6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let carried = |high: u16, low: u16| {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        public_v4(Ipv4Addr::new(a, b, c, d))
    };
    match segments {
        [0, 0, 0, 0, 0, 0xffff, high, low]
        | [0, 0, 0, 0, 0xffff, 0, high, low]
        | [0x64, 0xff9b, 0, 0, 0, 0, high, low]
        | [0x2002, high, low, ..] => carried(high, low),
        [0x2001, 0..=0x1ff | 0xdb8, ..] => false,
        [first, ..] => first & 0xe000 == 0x2000 && first & 0xfff0 != 0x3ff0,
    }
}

#[cfg(test)]
mod tests;

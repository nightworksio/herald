use std::fmt;

use herald_ports::HttpError;
use url::{Host, Url};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Origin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

impl Origin {
    fn of(url: &Url) -> Option<Self> {
        Some(Self {
            scheme: url.scheme().to_owned(),
            host: url.host_str()?.to_owned(),
            port: url.port_or_known_default(),
        })
    }
}

/// The origins one destination may reach: each HTTPS origin its config names,
/// and plain HTTP only on a loopback origin named explicitly (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach(Vec<Origin>);

/// A base URL a destination may not be given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachError {
    /// The base is not an absolute URL with a host.
    NotAUrl {
        /// The base as given.
        base: String,
    },
    /// The base is neither HTTPS nor HTTP on a loopback host.
    NotHttps {
        /// The base as given.
        base: String,
    },
}

impl fmt::Display for ReachError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAUrl { base } => write!(f, "{base} is not a URL with a host"),
            Self::NotHttps { base } => write!(
                f,
                "{base} is not HTTPS; plain HTTP is allowed only on a loopback host"
            ),
        }
    }
}

impl Reach {
    /// The origins of `bases`.
    ///
    /// # Errors
    ///
    /// The first base that is not a URL with a host, or that is plain HTTP on
    /// a host other than a loopback one.
    pub fn new<'a>(bases: impl IntoIterator<Item = &'a str>) -> Result<Self, ReachError> {
        bases
            .into_iter()
            .map(|base| {
                let parsed = Url::parse(base).ok();
                let Some((url, origin)) =
                    parsed.and_then(|url| Origin::of(&url).map(|origin| (url, origin)))
                else {
                    return Err(ReachError::NotAUrl {
                        base: base.to_owned(),
                    });
                };
                if url.scheme() == "https" || (url.scheme() == "http" && loopback(&url)) {
                    Ok(origin)
                } else {
                    Err(ReachError::NotHttps {
                        base: base.to_owned(),
                    })
                }
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// Whether `url` is on one of the origins.
    #[must_use]
    pub fn allows(&self, url: &Url) -> bool {
        Origin::of(url).is_some_and(|origin| self.0.contains(&origin))
    }
}

/// The domains herald's sessions hold connections under: Discord's gateway
/// and Slack's Socket Mode (ADR-0016).
pub const SESSIONS: [&str; 2] = ["discord.gg", "slack.com"];

/// What sockets may reach: `wss` under its domains alone, and plain `ws`
/// only on a loopback host (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketReach(Vec<String>);

impl SocketReach {
    /// Sockets reaching only hosts under `domains`.
    pub fn under<'a>(domains: impl IntoIterator<Item = &'a str>) -> Self {
        Self(domains.into_iter().map(ToOwned::to_owned).collect())
    }

    /// Whether `url` may be reached.
    #[must_use]
    pub fn allows(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        match url.scheme() {
            "wss" => self.0.iter().any(|domain| {
                host == domain
                    || host
                        .strip_suffix(domain.as_str())
                        .is_some_and(|rest| rest.ends_with('.'))
            }),
            "ws" => loopback(url),
            _ => false,
        }
    }

    /// The URL `url` names, where it may be reached.
    ///
    /// # Errors
    ///
    /// Where it may not, refused without its query, which can carry a
    /// ticket.
    pub fn admitted(&self, url: &str) -> Result<Url, HttpError> {
        Url::parse(url)
            .ok()
            .filter(|parsed| self.allows(parsed))
            .ok_or_else(|| HttpError::Refused {
                url: url.split('?').next().unwrap_or_default().to_owned(),
            })
    }
}

fn loopback(url: &Url) -> bool {
    url.host().is_some_and(|host| match host {
        Host::Domain(domain) => domain == "localhost",
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn https_and_loopback_http_bases_are_reachable_on_their_own_origins_only() {
        let reach = Reach::new([
            "https://discord.com/api/v10",
            "http://127.0.0.1:8080",
            "http://localhost:9",
            "http://[::1]:7",
        ])
        .unwrap();
        assert!(reach.allows(&url("https://discord.com/api/v10/channels/1/messages")));
        assert!(reach.allows(&url("https://discord.com:443/other")));
        assert!(reach.allows(&url("http://127.0.0.1:8080/x")));
        assert!(reach.allows(&url("http://localhost:9/")));
        assert!(reach.allows(&url("http://[::1]:7/")));
        assert!(!reach.allows(&url("http://discord.com/api/v10")));
        assert!(!reach.allows(&url("https://discord.com:8443/api")));
        assert!(!reach.allows(&url("https://discord.com.example.org/api")));
        assert!(!reach.allows(&url("https://discord.com@example.org/api")));
        assert!(!reach.allows(&url("http://127.0.0.1:8081/")));
        assert!(!reach.allows(&url("data:text/plain,discord.com")));
    }

    #[test]
    fn a_base_that_is_no_url_or_not_https_is_refused_by_name() {
        assert_eq!(
            Reach::new(["not a url"])
                .map(|_| ())
                .unwrap_err()
                .to_string(),
            "not a url is not a URL with a host"
        );
        assert_eq!(
            Reach::new(["http://discord.com"])
                .map(|_| ())
                .unwrap_err()
                .to_string(),
            "http://discord.com is not HTTPS; plain HTTP is allowed only on a loopback host"
        );
    }

    fn allowed(text: &str) -> bool {
        SocketReach::under(SESSIONS).allows(&url(text))
    }

    #[test]
    fn only_secure_sockets_under_the_domains_and_plain_ones_on_loopback_are_allowed() {
        assert!(allowed("wss://gateway.discord.gg/?v=10"));
        assert!(allowed("wss://gateway-us-east1-b.discord.gg"));
        assert!(allowed("wss://discord.gg"));
        assert!(allowed("wss://wss-primary.slack.com/link/?ticket=1"));
        assert!(!allowed("wss://evildiscord.gg"));
        assert!(!allowed("wss://discord.gg.evil.example"));
        assert!(!allowed("ws://gateway.discord.gg"));
        assert!(!allowed("https://gateway.discord.gg"));
        assert!(allowed("ws://127.0.0.1:9000"));
        assert!(allowed("ws://[::1]:9000"));
        assert!(allowed("ws://localhost:9000"));
        assert!(!allowed("ws://10.0.0.1"));
        assert!(!allowed("wss://127.0.0.1"));
        assert!(!allowed("data:text/plain,discord.gg"));
    }

    #[test]
    fn a_socket_not_allowed_is_refused_without_its_query() {
        let reach = SocketReach::under(SESSIONS);
        assert_eq!(
            reach.admitted("wss://evil.example/?ticket=secret"),
            Err(HttpError::Refused {
                url: "wss://evil.example/".to_owned()
            })
        );
        assert!(reach.admitted("not a url").is_err());
        assert_eq!(
            reach
                .admitted("wss://gateway.discord.gg/?v=10")
                .map(String::from),
            Ok("wss://gateway.discord.gg/?v=10".to_owned())
        );
    }

    #[test]
    fn plain_http_off_loopback_is_refused() {
        for base in [
            "http://discord.com",
            "http://10.0.0.1",
            "http://[fe80::1]",
            "ftp://127.0.0.1",
        ] {
            assert_eq!(
                Reach::new([base]),
                Err(ReachError::NotHttps {
                    base: base.to_owned()
                }),
                "{base}"
            );
        }
    }

    #[test]
    fn a_base_that_is_not_a_url_with_a_host_is_refused() {
        assert_eq!(
            Reach::new(["discord.com"]),
            Err(ReachError::NotAUrl {
                base: "discord.com".to_owned()
            })
        );
        for base in ["https:", "file:///etc/passwd", "data:text/plain,x"] {
            assert_eq!(
                Reach::new([base]),
                Err(ReachError::NotAUrl {
                    base: base.to_owned()
                }),
                "{base}"
            );
        }
    }

    #[test]
    fn each_error_reads_as_a_sentence() {
        assert_eq!(
            ReachError::NotAUrl {
                base: "x".to_owned()
            }
            .to_string(),
            "x is not a URL with a host"
        );
        assert_eq!(
            ReachError::NotHttps {
                base: "http://x".to_owned()
            }
            .to_string(),
            "http://x is not HTTPS; plain HTTP is allowed only on a loopback host"
        );
    }
}

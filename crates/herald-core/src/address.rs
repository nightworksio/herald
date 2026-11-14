//! What a config or an event names that goes into a URL: a web address, and
//! the names a path is built from. Every URL is read as WHATWG reads it, as
//! the HTTP clients that reach it do.

use std::ops::RangeInclusive;

use url::{Position, Url};

/// The schemes a web address is written with.
pub(crate) const WEB_SCHEMES: [&str; 2] = ["https://", "http://"];

/// A URL that names a host, read as WHATWG reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address(Url);

impl Address {
    /// `text` read as a URL, where it is one and names a host.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Url::parse(text)
            .ok()
            .filter(|url| url.host().is_some())
            .map(Self)
    }

    /// Its scheme, in lowercase.
    #[must_use]
    pub fn scheme(&self) -> &str {
        self.0.scheme()
    }

    /// Its host: a domain in lowercase, an IPv4 address, or an IPv6 address
    /// in brackets.
    #[must_use]
    pub fn host(&self) -> &str {
        self.0.host_str().unwrap_or_default()
    }

    /// Its port, or its scheme's where it names none.
    #[must_use]
    pub fn port(&self) -> Option<u16> {
        self.0.port_or_known_default()
    }

    /// Its host, and its port where that is not its scheme's: what a `Host`
    /// header carries.
    #[must_use]
    pub fn authority(&self) -> String {
        match self.0.port() {
            Some(port) => format!("{}:{port}", self.host()),
            None => self.host().to_owned(),
        }
    }

    /// Its scheme and authority.
    #[must_use]
    pub fn origin(&self) -> String {
        format!("{}://{}", self.scheme(), self.authority())
    }

    /// Its path and query: what a request line names.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.0[Position::BeforePath..Position::AfterQuery]
    }

    /// Whether it names a user or a password before its host.
    #[must_use]
    pub fn names_a_user(&self) -> bool {
        !self.0.username().is_empty() || self.0.password().is_some()
    }
}

/// Whether `url` is a web address: `https://` or `http://`, in any case,
/// then a host.
#[must_use]
pub fn web_url(url: &str) -> bool {
    let written = WEB_SCHEMES.iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|written| written.eq_ignore_ascii_case(scheme))
            && url
                .get(scheme.len()..)
                .is_some_and(|rest| !rest.starts_with(['/', '\\', '?', '#']))
    });
    written && Address::parse(url).is_some()
}

/// Whether `text` is a domain alone: two or more labels of lowercase ASCII
/// letters, digits and inner `-`, with no scheme, port or path.
#[must_use]
pub fn domain(text: &str) -> bool {
    let labels: Vec<&str> = text.split('.').collect();
    labels.len() > 1
        && labels.iter().all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

/// Whether `host` is `domain` or a host under it.
#[must_use]
pub fn under_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.'))
}

/// Whether `url` is a `wss://` URL whose host lies under one of `domains`,
/// with no credentials before it.
#[must_use]
pub fn socket_under(url: &str, domains: &[&str]) -> bool {
    Address::parse(url).is_some_and(|address| {
        address.scheme() == "wss"
            && !address.names_a_user()
            && domains
                .iter()
                .any(|domain| under_domain(address.host(), domain))
    })
}

/// Whether `path` is as many names as `names` allows, joined by `/`, each of
/// ASCII letters, digits, `.`, `_` and `-` and none of them `.` or `..`, so
/// it names one path and nothing else in a URL.
#[must_use]
pub fn path_of_names(path: &str, names: RangeInclusive<usize>) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    names.contains(&parts.len())
        && parts.iter().all(|name| {
            !name.is_empty()
                && *name != "."
                && *name != ".."
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_web_url_has_a_web_scheme_and_a_host() {
        for url in [
            "https://forge.example/api/v1",
            "http://127.0.0.1:8787/github",
            "HTTPS://Forge.example/x",
        ] {
            assert!(web_url(url), "{url}");
        }
        for url in [
            "ftp://forge.example",
            "https://",
            "https:///x",
            "http://?q",
            "https://[/",
            "https://a b/",
            "https:forge.example",
            "https:\\\\forge.example",
            "https://\\forge.example",
            "forge.example",
        ] {
            assert!(!web_url(url), "{url}");
        }
    }

    #[test]
    fn an_address_is_read_as_an_http_client_reads_it() {
        let cases = [
            (
                "https://github.com/cli/cli",
                "github.com",
                "github.com",
                "/cli/cli",
            ),
            (
                "https://user:secret@Forge.Example:8443/o/r?x=1#f",
                "forge.example",
                "forge.example:8443",
                "/o/r?x=1",
            ),
            ("https://[::1]:8/", "[::1]", "[::1]:8", "/"),
            ("http://127.0.0.1:80", "127.0.0.1", "127.0.0.1", "/"),
            (
                "https://dynamodb.eu-west-1.amazonaws.com:443",
                "dynamodb.eu-west-1.amazonaws.com",
                "dynamodb.eu-west-1.amazonaws.com",
                "/",
            ),
            (
                "https://evil.com\\.slack.com/",
                "evil.com",
                "evil.com",
                "/.slack.com/",
            ),
        ];
        for (url, host, authority, target) in cases {
            let address = Address::parse(url).unwrap();
            assert_eq!(address.host(), host, "{url}");
            assert_eq!(address.authority(), authority, "{url}");
            assert_eq!(address.target(), target, "{url}");
        }
        let address = Address::parse("https://u@h.example:8443/").unwrap();
        assert_eq!(address.scheme(), "https");
        assert_eq!(address.port(), Some(8443));
        assert_eq!(address.origin(), "https://h.example:8443");
        assert!(address.names_a_user());
        assert!(
            Address::parse("https://:p@h.example/")
                .unwrap()
                .names_a_user()
        );
        let plain = Address::parse("http://h.example/").unwrap();
        assert_eq!(plain.port(), Some(80));
        assert_eq!(plain.origin(), "http://h.example");
        assert!(!plain.names_a_user());
        for none in [
            "",
            "crates.io/api/v1",
            "mailto:a@b.example",
            "https://[/",
            "data:text/plain,x",
        ] {
            assert_eq!(Address::parse(none), None, "{none}");
        }
    }

    #[test]
    fn a_path_of_names_holds_nothing_that_moves_it() {
        assert!(path_of_names("nightworksio/herald", 2..=2));
        assert!(path_of_names("group/sub.group/my_name-2", 2..=20));
        assert!(path_of_names("caddy", 1..=2));
        for path in [
            "nightworksio",
            "a/b/c",
            "a/../b",
            "a/.",
            "a//b",
            "a/b?x=1",
            "a/b#x",
            "a/b%2F",
            "a/ b",
        ] {
            assert!(!path_of_names(path, 2..=2), "{path}");
        }
    }

    #[test]
    fn a_domain_is_lowercase_labels_with_nothing_around_them() {
        for good in ["slack.com", "gateway.discord.gg", "a-1.example"] {
            assert!(domain(good), "{good}");
        }
        for bad in [
            "localhost",
            "Slack.com",
            "slack.com:443",
            "https://slack.com",
            "slack..com",
            "-a.com",
            "a-.com",
            "slack.com/",
        ] {
            assert!(!domain(bad), "{bad}");
        }
    }

    #[test]
    fn a_socket_lies_under_a_domain_only_by_its_host() {
        let domains = ["slack.com", "discord.gg"];
        for good in [
            "wss://wss-primary.slack.com/link?ticket=1",
            "wss://slack.com",
            "wss://Resume.Discord.gg:443/?v=10",
        ] {
            assert!(socket_under(good, &domains), "{good}");
        }
        for bad in [
            "ws://slack.com/",
            "https://slack.com/",
            "wss://evilslack.com/",
            "wss://slack.com.evil.example/",
            "wss://slack.com@evil.example/",
            "wss://evil.example/slack.com",
            "wss://evil.example/?slack.com",
            "wss://[::1]:443/",
            "wss://evil.com\\.slack.com/",
            "wss://evil.com\\@slack.com/",
            "wss://user:pass@slack.com/",
        ] {
            assert!(!socket_under(bad, &domains), "{bad}");
        }
    }
}

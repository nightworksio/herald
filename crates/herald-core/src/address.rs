//! What a config or an event names that goes into a URL: a web address, and
//! the names a path is built from.

use std::ops::RangeInclusive;

/// The schemes a web address is written with.
pub const WEB_SCHEMES: [&str; 2] = ["https://", "http://"];

/// Whether `url` is a web address: `https://` or `http://`, then a host.
#[must_use]
pub fn web_url(url: &str) -> bool {
    WEB_SCHEMES.iter().any(|scheme| {
        url.strip_prefix(scheme)
            .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(['/', '?', '#']))
    })
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
    let Some(rest) = url.strip_prefix("wss://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
        .to_ascii_lowercase();
    domains.iter().any(|domain| under_domain(&host, domain))
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
        ] {
            assert!(web_url(url), "{url}");
        }
        for url in [
            "ftp://forge.example",
            "https://",
            "https:///x",
            "http://?q",
            "forge.example",
        ] {
            assert!(!web_url(url), "{url}");
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
        ] {
            assert!(!socket_under(bad, &domains), "{bad}");
        }
    }
}

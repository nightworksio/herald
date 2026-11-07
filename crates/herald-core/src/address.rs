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
}

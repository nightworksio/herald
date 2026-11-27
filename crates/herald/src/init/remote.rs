//! The repository's `origin` remote, read from its git config as a file:
//! no `git` process and no shell (ADR-0091 decision 3).

use std::path::{Path, PathBuf};

/// Where the `origin` remote is: the host of its forge and the
/// repository's path there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Remote {
    /// The web host, lowercase, without a port.
    pub(crate) host: String,
    /// The repository's path, such as `owner/name` or `group/sub/name`.
    pub(crate) path: String,
}

/// The `origin` remote the git config `text` names, where it names one
/// whose URL herald reads.
pub(crate) fn origin(text: &str) -> Option<Remote> {
    let mut within = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            within = section(line) == Some(("remote", "origin"));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if within && key.trim().eq_ignore_ascii_case("url") {
            return remote(unquoted(value.trim()));
        }
    }
    None
}

/// The section and subsection a git config header line names.
fn section(line: &str) -> Option<(&str, &str)> {
    let inner = line.strip_prefix('[')?.split(']').next()?.trim();
    let (name, sub) = inner.split_once(char::is_whitespace)?;
    let sub = sub.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some((name, sub)).filter(|(name, _)| name.eq_ignore_ascii_case("remote"))
}

/// `value` without the quotes around it, where it has them.
fn unquoted(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
}

/// The host and path a remote's URL names: `scheme://[user@]host[:port]/path`
/// or `[user@]host:path`.
fn remote(url: &str) -> Option<Remote> {
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        rest.split_once('/')?
    } else {
        url.split_once(':')
            .filter(|(host, _)| !host.contains('/'))?
    };
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?.to_ascii_lowercase();
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let names = path.split('/').collect::<Vec<_>>();
    if host.is_empty() || names.len() < 2 || names.iter().any(|name| name.is_empty()) {
        return None;
    }
    Some(Remote {
        host,
        path: path.to_owned(),
    })
}

/// The git config of the repository that holds `directory`: the nearest
/// `.git` at or above it, a directory or a worktree's file naming one.
pub(crate) fn config_of(directory: &Path) -> Option<PathBuf> {
    let directory = std::fs::canonicalize(directory).ok()?;
    directory.ancestors().find_map(|at| {
        let git = at.join(".git");
        if git.is_dir() {
            return Some(git.join("config"));
        }
        let named = std::fs::read_to_string(&git).ok()?;
        let gitdir = at.join(named.trim().strip_prefix("gitdir:")?.trim());
        let common = std::fs::read_to_string(gitdir.join("commondir"))
            .map_or_else(|_| gitdir.clone(), |common| gitdir.join(common.trim()));
        Some(common.join("config"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote_of(url: &str) -> Option<(String, String)> {
        origin(&format!(
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = {url}\n"
        ))
        .map(|remote| (remote.host, remote.path))
    }

    #[test]
    fn every_url_shape_git_takes_names_its_host_and_path() {
        for url in [
            "https://github.com/owner/app.git",
            "https://user@GitHub.com:443/owner/app/",
            "ssh://git@github.com:22/owner/app.git",
            "git@github.com:owner/app.git",
            "github.com:owner/app",
            "\"https://github.com/owner/app\"",
        ] {
            assert_eq!(
                remote_of(url),
                Some(("github.com".to_owned(), "owner/app".to_owned())),
                "{url}"
            );
        }
        assert_eq!(
            remote_of("https://gitlab.example/group/sub/app.git"),
            Some(("gitlab.example".to_owned(), "group/sub/app".to_owned()))
        );
    }

    #[test]
    fn a_url_naming_no_repository_is_not_read() {
        for url in [
            "https://github.com/owner",
            "https://github.com//app",
            "/srv/git/app.git",
            "../app",
            "https://@/owner/app",
        ] {
            assert_eq!(remote_of(url), None, "{url}");
        }
    }

    #[test]
    fn only_origin_s_url_is_read() {
        let text = "[remote \"upstream\"]\n\turl = https://github.com/up/app\n[branch \"main\"]\n\turl = x\n[remote \"origin\"]\n\tfetch = +refs/heads/*\n\turl = git@codeberg.org:me/app.git\n";
        assert_eq!(
            origin(text),
            Some(Remote {
                host: "codeberg.org".to_owned(),
                path: "me/app".to_owned()
            })
        );
        assert_eq!(
            origin("[remote \"upstream\"]\nurl = https://github.com/up/app\n"),
            None
        );
        assert_eq!(origin("[remote]\nurl = https://github.com/up/app\n"), None);
    }
}

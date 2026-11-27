//! The requests GitHub's REST API takes, from GitHub's own API or a GitHub
//! Enterprise Server's.

use herald_ports::{Method, Request, Secret, SecretText};

/// GitHub's own API, where no GitHub Enterprise Server's is named.
pub const API: &str = "https://api.github.com";

/// GitHub, as an error names it.
pub const GITHUB: &str = "GitHub";

/// The media type GitHub's REST API answers a file's raw bytes in.
pub const RAW: &str = "application/vnd.github.raw+json";

/// The version of GitHub's REST API herald was written against.
const VERSION: &str = "2022-11-28";

/// The base URL of the GitHub API at `given`, or of GitHub's own.
#[must_use]
pub fn api(given: Option<&str>) -> String {
    given.unwrap_or(API).trim_end_matches('/').to_owned()
}

/// The GraphQL endpoint beside the REST API at `api`: a GitHub Enterprise
/// Server's `https://ghe.example/api/v3` answers GraphQL at
/// `https://ghe.example/api/graphql`, and api.github.com and a GHE.com
/// tenant's `https://api.acme.ghe.com` at `/graphql`.
#[must_use]
pub fn graphql(api: &str) -> String {
    let api = api.trim_end_matches('/');
    match api.strip_suffix("/v3") {
        Some(base) => format!("{base}/graphql"),
        None => format!("{api}/graphql"),
    }
}

/// The host of GitHub's own container registry.
const GHCR: &str = "ghcr.io";

/// The host of the container registry beside the REST API at `api`:
/// ghcr.io beside api.github.com, `containers.acme.ghe.com` beside a GHE.com
/// tenant's `https://api.acme.ghe.com`, and `containers.ghe.example` beside
/// a GitHub Enterprise Server's `https://ghe.example/api/v3`.
#[must_use]
pub fn containers(api: &str) -> String {
    let api = api.trim_end_matches('/');
    if api == API {
        return GHCR.to_owned();
    }
    let host = herald_core::Address::parse(api)
        .map(|address| address.host().to_owned())
        .unwrap_or_default();
    let tenant = host
        .strip_prefix("api.")
        .filter(|tenant| tenant.ends_with(".ghe.com"));
    format!("containers.{}", tenant.unwrap_or(&host))
}

/// The committer github.com signs the commits it makes through its own API
/// for, with its own key.
const WEB_FLOW: &str = "noreply@github.com";

/// The committer the GitHub behind the REST API at `api` signs the commits
/// it makes through that API for, where herald knows it unasked:
/// github.com's beside api.github.com; none beside a GitHub Enterprise
/// Server's or a GHE.com tenant's, whose own a merge queue's `signer` names.
#[must_use]
pub fn web_flow(api: &str) -> Option<&'static str> {
    (api.trim_end_matches('/') == API).then_some(WEB_FLOW)
}

/// Whether the REST API at `api` is a GitHub Enterprise Server's, served
/// under `/api/v3`, and not github.com's or a GHE.com tenant's.
#[must_use]
pub fn server(api: &str) -> bool {
    api.trim_end_matches('/').ends_with("/v3")
}

/// `request` with the headers GitHub's REST API asks for, identified as
/// herald's.
#[must_use]
pub fn headed(request: Request) -> Request {
    request
        .accepting("application/vnd.github+json")
        .header("X-GitHub-Api-Version", SecretText::plain(VERSION))
        .identified()
}

/// A request to GitHub's REST API at `url`, carrying `token`.
#[must_use]
pub fn request(method: Method, url: String, token: Secret) -> Request {
    headed(Request::new(method, SecretText::plain(url))).bearer(token)
}

#[cfg(test)]
mod tests {
    use herald_ports::USER_AGENT;

    #[test]
    fn graphql_is_asked_beside_each_kind_of_github_s_rest_api() {
        for (rest, graphql) in [
            ("https://api.github.com", "https://api.github.com/graphql"),
            (
                "https://api.acme.ghe.com/",
                "https://api.acme.ghe.com/graphql",
            ),
            (
                "https://ghe.example/api/v3",
                "https://ghe.example/api/graphql",
            ),
            (
                "https://ghe.example/api/v3/",
                "https://ghe.example/api/graphql",
            ),
        ] {
            assert_eq!(super::graphql(rest), graphql, "{rest}");
        }
    }

    #[test]
    fn containers_are_named_on_the_registry_beside_each_kind_of_github_s_rest_api() {
        for (rest, registry) in [
            ("https://api.github.com", "ghcr.io"),
            ("https://api.github.com/", "ghcr.io"),
            ("https://api.acme.ghe.com", "containers.acme.ghe.com"),
            ("https://ghe.example/api/v3", "containers.ghe.example"),
            (
                "https://api.ghe.example/api/v3",
                "containers.api.ghe.example",
            ),
        ] {
            assert_eq!(super::containers(rest), registry, "{rest}");
        }
    }

    use super::*;

    #[test]
    fn github_com_alone_signs_for_a_committer_herald_knows_unasked() {
        for (rest, signer) in [
            ("https://api.github.com", Some("noreply@github.com")),
            ("https://api.github.com/", Some("noreply@github.com")),
            ("https://api.acme.ghe.com", None),
            ("https://ghe.example/api/v3", None),
        ] {
            assert_eq!(web_flow(rest), signer, "{rest}");
        }
    }

    #[test]
    fn a_server_alone_serves_its_api_under_v3() {
        for (rest, served) in [
            ("https://api.github.com", false),
            ("https://api.acme.ghe.com", false),
            ("https://ghe.example/api/v3", true),
            ("https://ghe.example/api/v3/", true),
        ] {
            assert_eq!(server(rest), served, "{rest}");
        }
    }

    #[test]
    fn the_api_is_the_one_given_or_github_s_own_without_a_trailing_slash() {
        assert_eq!(api(None), "https://api.github.com");
        assert_eq!(
            api(Some("https://ghe.example/api/v3/")),
            "https://ghe.example/api/v3"
        );
    }

    #[test]
    fn a_request_carries_github_s_headers_herald_s_agent_and_the_token() {
        let request = request(
            Method::Post,
            "https://api.github.com/repos/o/r".to_owned(),
            Secret::new("TOKEN", "hunter2"),
        );
        assert_eq!(
            request.to_string(),
            format!(
                "POST https://api.github.com/repos/o/r\nAccept: application/vnd.github+json\nX-GitHub-Api-Version: 2022-11-28\nUser-Agent: {USER_AGENT}\nAuthorization: Bearer «secret:TOKEN»"
            )
        );
    }
}

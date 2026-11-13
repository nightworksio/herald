//! The requests GitHub's REST API takes, from GitHub's own API or a GitHub
//! Enterprise Server's.

use herald_ports::{Method, Request, Secret, SecretText};

/// GitHub's own API, where no GitHub Enterprise Server's is named.
pub const API: &str = "https://api.github.com";

/// GitHub, as an error names it.
pub const GITHUB: &str = "GitHub";

/// The version of GitHub's REST API herald was written against.
const VERSION: &str = "2022-11-28";

/// The base URL of the GitHub API at `given`, or of GitHub's own.
#[must_use]
pub fn api(given: Option<&str>) -> String {
    given.unwrap_or(API).trim_end_matches('/').to_owned()
}

/// `request` with the headers GitHub's REST API asks for, identified as
/// herald's.
#[must_use]
pub fn headed(request: Request) -> Request {
    request
        .header("Accept", SecretText::plain("application/vnd.github+json"))
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

    use super::*;

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

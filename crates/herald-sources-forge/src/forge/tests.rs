use super::*;

#[test]
fn each_forge_signs_under_its_own_scheme() {
    assert_eq!(
        [Forge::GitHub, Forge::GitLab, Forge::Forgejo].map(|forge| forge.scheme().name),
        ["github", "gitlab", "forgejo"]
    );
}

#[test]
fn each_forge_is_named_as_a_source_s_kind_writes_it() {
    assert_eq!(Forge::ALL.map(Forge::kind), ["github", "gitlab", "forgejo"]);
}

#[test]
fn a_forge_s_repositories_are_named_on_its_api_s_host_without_github_s_api_label() {
    for (forge, api, host) in [
        (Forge::GitHub, "https://api.github.com", "github.com"),
        (Forge::GitHub, "https://api.acme.ghe.com", "acme.ghe.com"),
        (
            Forge::GitHub,
            "https://ghe.example:8443/api/v3",
            "ghe.example:8443",
        ),
        (Forge::GitLab, "https://gitlab.com/api/v4", "gitlab.com"),
        (
            Forge::Forgejo,
            "https://api.forge.example/api/v1",
            "api.forge.example",
        ),
    ] {
        assert_eq!(host_of(forge, api), host, "{api}");
    }
    assert_eq!(host_of(Forge::GitLab, "not a url"), "");
    let origin = Origin::new(Forge::GitHub, &crate::SourceOptions::default(), "cli/cli");
    assert_eq!(origin.host(), "github.com");
}

#[test]
fn each_forge_takes_a_repository_in_its_shape_and_an_api_at_a_web_address() {
    let options = |repository: &str, api: Option<&str>| crate::SourceOptions {
        repository: Some(repository.to_owned()),
        api: api.map(ToOwned::to_owned),
        ..crate::SourceOptions::default()
    };
    for forge in Forge::ALL {
        assert_eq!(
            options("nightworksio/herald", Some("https://forge.example/api/v1")).check(forge),
            Ok(())
        );
        assert_eq!(
            options("a/b", Some("ftp://forge.example")).check(forge),
            Err("\"ftp://forge.example\" is not a web address; give the API's base URL, such as https://forge.example/api/v1".to_owned())
        );
    }
    assert_eq!(options("group/sub/name", None).check(Forge::GitLab), Ok(()));
    assert!(
        options("group/sub/name", None)
            .check(Forge::GitHub)
            .is_err()
    );
    for (forge, shape) in [
        (Forge::GitHub, "owner/name"),
        (
            Forge::GitLab,
            "the project's full path, such as group/subgroup/name",
        ),
        (Forge::Forgejo, "owner/name"),
    ] {
        assert_eq!(
            options("a/../b", None).check(forge),
            Err(format!(
                "\"a/../b\" is not a repository; give it as {shape}, of letters, digits, '.', '_' and '-'"
            ))
        );
    }
}

#[test]
fn a_repository_or_release_read_answered_with_a_rate_limit_says_it_is_one() {
    let limited = Response {
        status: 403,
        headers: vec![("Retry-After".to_owned(), "30".to_owned())],
        body: Vec::new(),
    };
    let repository = Repository {
        id: 1,
        name: "herald".to_owned(),
        page: None,
    };
    let rule = PrereleaseRule::default();
    for forge in Forge::ALL {
        let api = forge.api();
        for (what, error) in [
            (api.repository, forge.repository(&limited).unwrap_err()),
            (
                api.release,
                forge.release(&repository, &limited, rule).unwrap_err(),
            ),
            (
                api.releases,
                forge
                    .releases(&repository, &limited, (1 << 20, rule))
                    .unwrap_err(),
            ),
        ] {
            assert_eq!(
                error,
                SourceError::RateLimited {
                    reason: format!(
                        "{what} could not be read: the forge's rate limit was reached; the limit resets in 30s"
                    )
                },
                "{forge:?}"
            );
        }
    }
}

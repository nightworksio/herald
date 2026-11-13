//! GitHub's release event and releases API, read from what GitHub sends.

#[cfg(test)]
mod github {
    use herald_core::{Cursor, Kind, Polled, Problem, prerelease_version};
    use herald_ports::{Response, Secret};
    use herald_sources_forge::{Forge, Hooked, Origin, Repository, SourceOptions, Token};
    use herald_wire::SourceError;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    const EVENT: &str = include_str!("../fixtures/github/release-event.reference.json");
    const RELEASES: &str = include_str!("../fixtures/github/releases.json");
    const REPOSITORY: &str = include_str!("../fixtures/github/repository.json");

    fn edited(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut payload: Value = serde_json::from_str(EVENT).unwrap();
        edit(&mut payload);
        serde_json::to_vec(&payload).unwrap()
    }

    fn answer(status: u16, body: &str) -> Response {
        Response {
            status,
            headers: vec![
                ("ETag".to_owned(), "W/\"abc\"".to_owned()),
                (
                    "Last-Modified".to_owned(),
                    "Sat, 31 Oct 2026 05:17:30 GMT".to_owned(),
                ),
            ],
            body: body.as_bytes().to_vec(),
        }
    }

    fn neovim() -> Repository {
        Forge::GitHub.repository(&answer(200, REPOSITORY)).unwrap()
    }

    fn source(api: Option<&str>) -> Origin {
        Origin::new(
            Forge::GitHub,
            &SourceOptions {
                repository: "neovim/neovim".to_owned(),
                api: api.map(ToOwned::to_owned),
                ..SourceOptions::default()
            },
        )
    }

    const RELEASE: (&str, &str) = ("X-GitHub-Event", "release");

    #[test]
    fn a_webhook_of_the_source_s_published_release_announces_it() {
        let hooked = Forge::GitHub.hooked(&[RELEASE], EVENT.as_bytes(), "CLI/cli");
        assert_eq!(
            hooked,
            Ok(Hooked::Event(
                Forge::GitHub.event(EVENT.as_bytes()).unwrap().unwrap()
            ))
        );
    }

    #[test]
    fn a_webhook_of_another_event_or_an_unpublished_release_announces_nothing() {
        for headers in [&[("X-GitHub-Event", "ping")][..], &[]] {
            assert_eq!(
                Forge::GitHub.hooked(headers, b"{\"zen\":\"x\"}", "cli/cli"),
                Ok(Hooked::Nothing("the delivery is not of a release"))
            );
        }
        let created = edited(|payload| payload["action"] = json!("created"));
        assert_eq!(
            Forge::GitHub.hooked(&[RELEASE], &created, "cli/cli"),
            Ok(Hooked::Nothing(
                "the release is not published, or is a draft"
            ))
        );
    }

    #[test]
    fn the_identity_names_the_host_of_the_release_s_page_as_a_client_reads_it() {
        let page = |url: &str| edited(|payload| payload["release"]["html_url"] = json!(url));
        let read = Forge::GitHub
            .event(&page(
                "https://bot:secret@GitHub.Example:8443/o/r/releases/tag/v1",
            ))
            .unwrap()
            .unwrap();
        assert!(read.identity().as_str().starts_with("github.example/"));
        let bracketed = Forge::GitHub
            .event(&page("https://[2001:db8::1]/o/r/releases/tag/v1"))
            .unwrap()
            .unwrap();
        assert!(bracketed.identity().as_str().starts_with("[2001:db8::1]/"));
        assert_eq!(
            Forge::GitHub
                .event(&page("o/r/releases/tag/v1"))
                .unwrap_err()
                .to_string(),
            SourceError::Unreadable {
                what: "GitHub's release event",
                reason: "a release's page names no host".to_owned(),
            }
            .to_string()
        );
    }

    #[test]
    fn a_webhook_of_another_repository_or_of_no_release_is_refused() {
        let elsewhere = Forge::GitHub.hooked(&[RELEASE], EVENT.as_bytes(), "other/repo");
        assert_eq!(
            elsewhere.unwrap_err().to_string(),
            SourceError::Unreadable {
                what: "GitHub's release event",
                reason: "it is not of other/repo, the repository its source names".to_owned(),
            }
            .to_string()
        );
        let unnamed = edited(|payload| payload["repository"]["full_name"] = json!(7));
        assert_eq!(
            Forge::GitHub
                .hooked(&[RELEASE], &unnamed, "cli/cli")
                .unwrap_err()
                .to_string(),
            "GitHub's release event could not be read: it is not of cli/cli, the repository its source names"
        );
        assert_eq!(
            Forge::GitHub
                .hooked(&[RELEASE], b"{}", "cli/cli")
                .unwrap_err()
                .to_string(),
            "GitHub's release event could not be read: missing field `action` at line 1 column 2"
        );
    }

    #[test]
    fn a_published_release_event_becomes_an_event() {
        let event = Forge::GitHub.event(EVENT.as_bytes()).unwrap().unwrap();
        assert_eq!(event.identity().as_str(), "github.com/212613049/v2.103.3");
        assert_eq!(event.kind(), &Kind::RELEASE);
        assert_eq!(event.project(), "cli");
        assert_eq!(event.version(), Some("v2.103.3"));
        assert_eq!(event.title(), "GitHub CLI 2.103.3");
        assert!(event.body().starts_with("## What's Changed"));
        assert_eq!(
            event.link("release"),
            Some("https://github.com/cli/cli/releases/tag/v2.103.3")
        );
        assert!(!event.prerelease());
        assert_eq!(event.occurred().to_string(), "2026-10-26T19:20:08Z");
    }

    #[test]
    fn only_a_published_release_that_is_not_a_draft_announces() {
        let created = edited(|payload| payload["action"] = json!("created"));
        assert_eq!(Forge::GitHub.event(&created), Ok(None));
        let draft = edited(|payload| payload["release"]["draft"] = json!(true));
        assert_eq!(Forge::GitHub.event(&draft), Ok(None));
    }

    #[test]
    fn a_tag_with_a_pre_release_part_is_a_pre_release_and_a_nameless_release_takes_its_tag() {
        let candidate = edited(|payload| {
            payload["release"]["tag_name"] = json!("v3.0.0-rc.1");
            payload["release"]["name"] = json!("  ");
            payload["release"]["body"] = json!(null);
        });
        let event = Forge::GitHub.event(&candidate).unwrap().unwrap();
        assert!(event.prerelease());
        assert_eq!(event.title(), "v3.0.0-rc.1");
        assert_eq!(event.body(), "");
    }

    #[test]
    fn what_is_not_a_release_event_says_why() {
        let unpublished = edited(|payload| payload["release"]["published_at"] = json!(null));
        assert_eq!(
            Forge::GitHub.event(&unpublished).unwrap_err().to_string(),
            "GitHub's release event could not be read: a published release has no publication time"
        );
        let nameless = edited(|payload| payload["repository"]["name"] = json!(""));
        assert_eq!(
            Forge::GitHub.event(&nameless),
            Err(SourceError::Invalid {
                what: "GitHub's release event",
                problem: Problem::NoProject
            })
        );
        let error = Forge::GitHub
            .event(b"{\"action\":\"published\"}")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "GitHub's release event could not be read: missing field `release` at line 1 column 22"
        );
        let huge = vec![b' '; 4_194_305];
        assert_eq!(
            Forge::GitHub.event(&huge).unwrap_err().to_string(),
            "GitHub's release event is larger than the 4194304 bytes herald reads"
        );
        assert_eq!(
            Forge::GitHub
                .event(&huge[..4_194_304])
                .unwrap_err()
                .to_string(),
            "GitHub's release event could not be read: EOF while parsing a value at line 1 column 4194304"
        );
    }

    #[test]
    fn a_poll_finds_the_published_releases_newest_first_with_the_validators() {
        assert_eq!(
            neovim(),
            Repository {
                id: 16_408_992,
                name: "neovim".to_owned()
            }
        );
        let Polled {
            events,
            etag,
            last_modified,
        } = Forge::GitHub
            .releases(&neovim(), &answer(200, RELEASES), herald_wire::PARSE_LIMIT)
            .unwrap();
        let found: Vec<(&str, bool)> = events
            .iter()
            .map(|event| (event.identity().as_str(), event.prerelease()))
            .collect();
        assert_eq!(
            found,
            [
                ("github.com/16408992/nightly", true),
                ("github.com/16408992/v0.12.6", false),
                ("github.com/16408992/stable", false),
            ]
        );
        assert_eq!(etag.as_deref(), Some("W/\"abc\""));
        assert_eq!(
            last_modified.as_deref(),
            Some("Sat, 31 Oct 2026 05:17:30 GMT")
        );
    }

    #[test]
    fn one_release_is_asked_for_by_its_tag_and_becomes_its_event() {
        let request = source(None).release_request("v0.12.6", None);
        assert_eq!(
            request.url.redacted(),
            "https://api.github.com/repos/neovim/neovim/releases/tags/v0.12.6"
        );
        let odd = source(None).release_request("release/1 2", None);
        assert_eq!(
            odd.url.redacted(),
            "https://api.github.com/repos/neovim/neovim/releases/tags/release%2F1%202"
        );
        let releases: Value = serde_json::from_str(RELEASES).unwrap();
        let one = releases[1].to_string();
        let event = Forge::GitHub
            .release(&neovim(), &answer(200, &one))
            .unwrap()
            .unwrap();
        assert_eq!(event.identity().as_str(), "github.com/16408992/v0.12.6");
        let mut draft = releases[1].clone();
        draft["draft"] = json!(true);
        assert_eq!(
            Forge::GitHub.release(&neovim(), &answer(200, &draft.to_string())),
            Ok(None)
        );
        assert_eq!(
            Forge::GitHub
                .release(&neovim(), &answer(404, "{}"))
                .unwrap_err()
                .to_string(),
            "GitHub's release was answered with 404"
        );
    }

    #[test]
    fn a_poll_with_nothing_new_or_a_refusal_says_so() {
        assert!(Forge::unchanged(&answer(304, "")));
        assert!(!Forge::unchanged(&answer(200, RELEASES)));
        assert_eq!(
            Forge::GitHub
                .releases(&neovim(), &answer(403, "{}"), herald_wire::PARSE_LIMIT)
                .unwrap_err()
                .to_string(),
            "GitHub's releases was answered with 403"
        );
        assert_eq!(
            Forge::GitHub.repository(&answer(404, "{}")),
            Err(SourceError::Answered {
                what: "GitHub's repository",
                status: 404
            })
        );
        let mut drafts: Value = serde_json::from_str(RELEASES).unwrap();
        drafts[0]["draft"] = json!(true);
        drafts[1]["published_at"] = json!(null);
        let body = drafts.to_string();
        assert_eq!(
            Forge::GitHub
                .releases(&neovim(), &answer(200, &body), herald_wire::PARSE_LIMIT)
                .unwrap_err()
                .to_string(),
            "GitHub's releases could not be read: a published release has no publication time"
        );
        drafts[1]["draft"] = json!(true);
        let body = drafts.to_string();
        let releases = Forge::GitHub
            .releases(&neovim(), &answer(200, &body), herald_wire::PARSE_LIMIT)
            .unwrap();
        assert_eq!(releases.events.len(), 1);
    }

    #[test]
    fn the_requests_ask_conditionally_with_herald_s_user_agent() {
        let token = Token::Run(Secret::new("GITHUB_TOKEN", "hunter2"));
        let cursor = Cursor {
            seen: Vec::new(),
            etag: Some("W/\"abc\"".to_owned()),
            last_modified: Some("Sat, 31 Oct 2026 05:17:30 GMT".to_owned()),
        };
        let request = source(None).releases_request(Some(&cursor), Some(&token));
        assert_eq!(
            request.to_string(),
            [
                "GET https://api.github.com/repos/neovim/neovim/releases?per_page=30",
                "Accept: application/vnd.github+json",
                "X-GitHub-Api-Version: 2022-11-28",
                "User-Agent: herald/0.0.0 (+https://github.com/nightworksio/herald)",
                "Authorization: Bearer «secret:GITHUB_TOKEN»",
                "If-None-Match: W/\"abc\"",
                "If-Modified-Since: Sat, 31 Oct 2026 05:17:30 GMT",
            ]
            .join("\n")
        );
        assert_eq!(request.most, herald_wire::PARSE_LIMIT);
        let enterprise = source(Some("https://ghe.example/api/v3/"));
        assert_eq!(enterprise.api(), "https://ghe.example/api/v3");
        assert_eq!(source(None).api(), "https://api.github.com");
        let plain = enterprise.releases_request(None, None);
        assert_eq!(
            plain.url.redacted(),
            "https://ghe.example/api/v3/repos/neovim/neovim/releases?per_page=30"
        );
        assert_eq!(plain.headers.len(), 3);
        assert_eq!(
            source(Some("https://api.acme.ghe.com"))
                .releases_request(None, None)
                .url
                .redacted(),
            "https://api.acme.ghe.com/repos/neovim/neovim/releases?per_page=30"
        );
        assert_eq!(
            source(None).repository_request(None).url.redacted(),
            "https://api.github.com/repos/neovim/neovim"
        );
    }

    fn payload(tag: &str, host: &str, id: u64, prerelease: bool) -> Vec<u8> {
        edited(|payload| {
            payload["release"]["tag_name"] = json!(tag);
            payload["release"]["prerelease"] = json!(prerelease);
            payload["release"]["html_url"] =
                json!(format!("https://{host}:8443/o/r/releases/tag/{tag}"));
            payload["repository"]["id"] = json!(id);
        })
    }

    proptest! {
        #[test]
        fn any_bytes_are_an_event_nothing_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = Forge::GitHub.event(&bytes);
            let _ = Forge::GitHub.releases(&neovim(), &Response { status: 200, headers: Vec::new(), body: bytes.clone() }, herald_wire::PARSE_LIMIT);
            let _ = Forge::GitHub.repository(&Response { status: 200, headers: Vec::new(), body: bytes });
        }

        #[test]
        fn the_identity_is_the_host_the_repository_id_and_the_tag(
            tag in "[a-z0-9.+-]{1,20}",
            host in "[a-z]{1,10}\\.example",
            id in any::<u64>(),
            flagged in any::<bool>(),
        ) {
            let event = Forge::GitHub.event(&payload(&tag, &host, id, flagged)).unwrap().unwrap();
            prop_assert_eq!(event.identity().as_str(), format!("{host}/{id}/{tag}"));
            prop_assert_eq!(event.prerelease(), flagged || prerelease_version(&tag));
        }
    }
}

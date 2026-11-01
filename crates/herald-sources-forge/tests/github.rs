//! GitHub's release event and releases API, read from what GitHub sends.

#[cfg(test)]
mod github {
    use herald_core::{Cursor, Kind, Problem, prerelease_version};
    use herald_ports::{Response, Secret};
    use herald_sources_forge::{ForgeError, GitHub, Polled, Repository, SourceOptions};
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
        GitHub::repository(&answer(200, REPOSITORY)).unwrap()
    }

    fn source(api: Option<&str>) -> GitHub {
        GitHub::new(&SourceOptions {
            repository: "neovim/neovim".to_owned(),
            api: api.map(ToOwned::to_owned),
            ..SourceOptions::default()
        })
    }

    #[test]
    fn a_published_release_event_becomes_an_event() {
        let event = GitHub::event(EVENT.as_bytes()).unwrap().unwrap();
        assert_eq!(event.identity().as_str(), "github.com/212613049/v2.103.3");
        assert_eq!(event.kind(), Kind::Release);
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
        assert_eq!(GitHub::event(&created), Ok(None));
        let draft = edited(|payload| payload["release"]["draft"] = json!(true));
        assert_eq!(GitHub::event(&draft), Ok(None));
    }

    #[test]
    fn a_tag_with_a_pre_release_part_is_a_pre_release_and_a_nameless_release_takes_its_tag() {
        let candidate = edited(|payload| {
            payload["release"]["tag_name"] = json!("v3.0.0-rc.1");
            payload["release"]["name"] = json!("  ");
            payload["release"]["body"] = json!(null);
        });
        let event = GitHub::event(&candidate).unwrap().unwrap();
        assert!(event.prerelease());
        assert_eq!(event.title(), "v3.0.0-rc.1");
        assert_eq!(event.body(), "");
    }

    #[test]
    fn what_is_not_a_release_event_says_why() {
        let unpublished = edited(|payload| payload["release"]["published_at"] = json!(null));
        assert_eq!(
            GitHub::event(&unpublished).unwrap_err().to_string(),
            "GitHub's release event could not be read: a published release has no published_at"
        );
        let nameless = edited(|payload| payload["repository"]["name"] = json!(""));
        assert_eq!(
            GitHub::event(&nameless),
            Err(ForgeError::Invalid {
                what: "GitHub's release event",
                problem: Problem::NoProject
            })
        );
        let error = GitHub::event(b"{\"action\":\"published\"}").unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("GitHub's release event could not be read: missing field `release`"),
            "{error}"
        );
        let huge = vec![b' '; 4_194_305];
        assert_eq!(
            GitHub::event(&huge).unwrap_err().to_string(),
            "GitHub's release event is larger than the 4194304 bytes herald reads"
        );
        assert!(GitHub::event(&huge[..4_194_304]).is_err());
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
        let Polled::Found {
            events,
            etag,
            last_modified,
        } = GitHub::releases(&neovim(), &answer(200, RELEASES)).unwrap()
        else {
            panic!("nothing found");
        };
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
    fn a_poll_with_nothing_new_or_a_refusal_says_so() {
        assert_eq!(
            GitHub::releases(&neovim(), &answer(304, "")),
            Ok(Polled::Unchanged)
        );
        assert_eq!(
            GitHub::releases(&neovim(), &answer(403, "{}"))
                .unwrap_err()
                .to_string(),
            "GitHub's releases was answered with 403"
        );
        assert_eq!(
            GitHub::repository(&answer(404, "{}")),
            Err(ForgeError::Answered {
                what: "GitHub's repository",
                status: 404
            })
        );
        let mut drafts: Value = serde_json::from_str(RELEASES).unwrap();
        drafts[0]["draft"] = json!(true);
        drafts[1]["published_at"] = json!(null);
        let body = drafts.to_string();
        assert!(GitHub::releases(&neovim(), &answer(200, &body)).is_err());
        drafts[1]["draft"] = json!(true);
        let body = drafts.to_string();
        let Ok(Polled::Found { events, .. }) = GitHub::releases(&neovim(), &answer(200, &body))
        else {
            panic!("the drafts were not skipped");
        };
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn the_requests_ask_conditionally_with_herald_s_user_agent() {
        let token = Secret::new("GITHUB_TOKEN", "hunter2");
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
        let plain = source(Some("https://ghe.example/api/v3/")).releases_request(None, None);
        assert_eq!(
            plain.url.redacted(),
            "https://ghe.example/api/v3/repos/neovim/neovim/releases?per_page=30"
        );
        assert_eq!(plain.headers.len(), 3);
        assert_eq!(
            source(None).repository_request(None).url.redacted(),
            "https://api.github.com/repos/neovim/neovim"
        );
    }

    #[test]
    fn each_error_reads_as_a_sentence() {
        assert_eq!(
            ForgeError::Invalid {
                what: "x",
                problem: Problem::NoTitle
            }
            .to_string(),
            "x makes no event: the event has no title"
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
            let _ = GitHub::event(&bytes);
            let _ = GitHub::releases(&neovim(), &Response { status: 200, headers: Vec::new(), body: bytes.clone() });
            let _ = GitHub::repository(&Response { status: 200, headers: Vec::new(), body: bytes });
        }

        #[test]
        fn the_identity_is_the_host_the_repository_id_and_the_tag(
            tag in "[a-z0-9.+-]{1,20}",
            host in "[a-z]{1,10}\\.example",
            id in any::<u64>(),
            flagged in any::<bool>(),
        ) {
            let event = GitHub::event(&payload(&tag, &host, id, flagged)).unwrap().unwrap();
            prop_assert_eq!(event.identity().as_str(), format!("{host}/{id}/{tag}"));
            prop_assert_eq!(event.prerelease(), flagged || prerelease_version(&tag));
        }
    }
}

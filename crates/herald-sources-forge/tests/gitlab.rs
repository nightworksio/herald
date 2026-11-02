//! GitLab's release hook and releases API, read from what GitLab sends.

#[cfg(test)]
mod gitlab {
    use herald_core::{Cursor, Kind};
    use herald_ports::{Response, Secret};
    use herald_sources_forge::{Forge, Origin, Repository, SourceOptions, Token};
    use herald_wire::SourceError;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    const HOOK: &str = include_str!("../fixtures/gitlab/release-hook.reference.json");
    const RELEASES: &str = include_str!("../fixtures/gitlab/releases.json");
    const PROJECT: &str = include_str!("../fixtures/gitlab/project.json");

    fn edited(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut payload: Value = serde_json::from_str(HOOK).unwrap();
        edit(&mut payload);
        serde_json::to_vec(&payload).unwrap()
    }

    fn answer(status: u16, body: &str) -> Response {
        Response {
            status,
            headers: vec![(
                "ETag".to_owned(),
                "W/\"638d6739141e7711a8b7d5d5f7cdd0fa\"".to_owned(),
            )],
            body: body.as_bytes().to_vec(),
        }
    }

    fn cli() -> Repository {
        Forge::GitLab.repository(&answer(200, PROJECT)).unwrap()
    }

    fn source(api: Option<&str>) -> Origin {
        Origin::new(
            Forge::GitLab,
            &SourceOptions {
                repository: "gitlab-org/cli".to_owned(),
                api: api.map(ToOwned::to_owned),
                ..SourceOptions::default()
            },
        )
    }

    #[test]
    fn a_created_release_hook_becomes_an_event() {
        let event = Forge::GitLab.event(HOOK.as_bytes()).unwrap().unwrap();
        assert_eq!(event.identity().as_str(), "gitlab.com/34675721/v1.126.0");
        assert_eq!(event.kind(), Kind::Release);
        assert_eq!(event.project(), "cli");
        assert_eq!(event.version(), Some("v1.126.0"));
        assert_eq!(event.title(), "v1.126.0");
        assert!(event.body().starts_with("## Changelog"));
        assert_eq!(
            event.link("release"),
            Some("https://gitlab.com/gitlab-org/cli/-/releases/v1.126.0")
        );
        assert!(!event.prerelease());
        assert_eq!(event.occurred().to_string(), "2026-10-28T21:42:40Z");
    }

    #[test]
    fn only_a_created_release_announces_and_a_tag_says_whether_it_is_a_pre_release() {
        for action in ["update", "delete"] {
            let changed = edited(|payload| payload["action"] = json!(action));
            assert_eq!(Forge::GitLab.event(&changed), Ok(None));
        }
        let candidate = edited(|payload| {
            payload["tag"] = json!("v2.0.0-rc.1");
            payload["released_at"] = json!("2026-10-28T21:42:40.023Z");
        });
        let event = Forge::GitLab.event(&candidate).unwrap().unwrap();
        assert!(event.prerelease());
        assert_eq!(event.occurred().to_string(), "2026-10-28T21:42:40.023Z");
    }

    #[test]
    fn what_is_not_a_release_hook_says_why() {
        let pushed = edited(|payload| payload["object_kind"] = json!("push"));
        assert_eq!(
            Forge::GitLab.event(&pushed).unwrap_err().to_string(),
            "GitLab's release event could not be read: it is a push hook, not a release hook"
        );
        let unreleased = edited(|payload| payload["released_at"] = json!(null));
        assert_eq!(
            Forge::GitLab.event(&unreleased).unwrap_err().to_string(),
            "GitLab's release event could not be read: a published release has no publication time"
        );
        let missing = edited(|payload| {
            payload.as_object_mut().unwrap().remove("released_at");
        });
        assert_eq!(
            Forge::GitLab.event(&missing).unwrap_err().to_string(),
            "GitLab's release event could not be read: a published release has no publication time"
        );
        let garbled = edited(|payload| payload["released_at"] = json!("last Tuesday UTC"));
        assert!(matches!(
            Forge::GitLab.event(&garbled),
            Err(SourceError::Unreadable { .. })
        ));
        let garbled = edited(|payload| payload["released_at"] = json!("last Tuesday"));
        assert!(matches!(
            Forge::GitLab.event(&garbled),
            Err(SourceError::Unreadable { .. })
        ));
    }

    #[test]
    fn a_poll_finds_the_released_releases_newest_first_with_the_validators() {
        assert_eq!(
            cli(),
            Repository {
                id: 34_675_721,
                name: "cli".to_owned()
            }
        );
        let releases = Forge::GitLab
            .releases(&cli(), &answer(200, RELEASES))
            .unwrap();
        let found: Vec<&str> = releases
            .events
            .iter()
            .map(|event| event.identity().as_str())
            .collect();
        assert_eq!(
            found,
            [
                "gitlab.com/34675721/v1.126.0",
                "gitlab.com/34675721/v1.125.0",
                "gitlab.com/34675721/v1.124.0",
            ]
        );
        assert_eq!(
            releases.etag.as_deref(),
            Some("W/\"638d6739141e7711a8b7d5d5f7cdd0fa\"")
        );
        assert_eq!(releases.last_modified, None);
    }

    #[test]
    fn an_upcoming_release_waits_until_it_is_released() {
        let mut upcoming: Value = serde_json::from_str(RELEASES).unwrap();
        upcoming[0]["upcoming_release"] = json!(true);
        let releases = Forge::GitLab
            .releases(&cli(), &answer(200, &upcoming.to_string()))
            .unwrap();
        assert_eq!(releases.events.len(), 2);
        assert_eq!(
            Forge::GitLab.release(&cli(), &answer(200, &upcoming[0].to_string())),
            Ok(None)
        );
    }

    #[test]
    fn one_release_is_asked_for_by_its_tag_and_becomes_its_event() {
        let request = source(None).release_request("v1.126.0", None);
        assert_eq!(
            request.url.redacted(),
            "https://gitlab.com/api/v4/projects/gitlab-org%2Fcli/releases/v1.126.0"
        );
        let releases: Value = serde_json::from_str(RELEASES).unwrap();
        let event = Forge::GitLab
            .release(&cli(), &answer(200, &releases[0].to_string()))
            .unwrap()
            .unwrap();
        assert_eq!(event.identity().as_str(), "gitlab.com/34675721/v1.126.0");
        assert_eq!(
            Forge::GitLab
                .release(&cli(), &answer(404, "{}"))
                .unwrap_err()
                .to_string(),
            "GitLab's release was answered with 404"
        );
        assert_eq!(
            Forge::GitLab.repository(&answer(404, "{}")),
            Err(SourceError::Answered {
                what: "GitLab's project",
                status: 404
            })
        );
    }

    #[test]
    fn a_named_token_is_a_bearer_and_the_run_s_own_is_a_job_token() {
        let cursor = Cursor {
            seen: Vec::new(),
            etag: Some("W/\"abc\"".to_owned()),
            last_modified: None,
        };
        let named = Token::Named(Secret::new("GITLAB_TOKEN", "hunter2"));
        assert_eq!(
            source(None)
                .releases_request(Some(&cursor), Some(&named))
                .to_string(),
            [
                "GET https://gitlab.com/api/v4/projects/gitlab-org%2Fcli/releases?per_page=30",
                "Accept: application/json",
                "User-Agent: herald/0.0.0 (+https://github.com/nightworksio/herald)",
                "Authorization: Bearer «secret:GITLAB_TOKEN»",
                "If-None-Match: W/\"abc\"",
            ]
            .join("\n")
        );
        let run = Token::Run(Secret::new("CI_JOB_TOKEN", "hunter2"));
        let managed = source(Some("https://gitlab.example/api/v4/"));
        assert_eq!(managed.api(), "https://gitlab.example/api/v4");
        assert_eq!(managed.forge(), Forge::GitLab);
        assert_eq!(
            managed.repository_request(Some(&run)).to_string(),
            [
                "GET https://gitlab.example/api/v4/projects/gitlab-org%2Fcli",
                "Accept: application/json",
                "User-Agent: herald/0.0.0 (+https://github.com/nightworksio/herald)",
                "JOB-TOKEN: «secret:CI_JOB_TOKEN»",
            ]
            .join("\n")
        );
    }

    proptest! {
        #[test]
        fn any_bytes_are_an_event_nothing_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = Forge::GitLab.event(&bytes);
            let response = Response { status: 200, headers: Vec::new(), body: bytes };
            let _ = Forge::GitLab.releases(&cli(), &response);
            let _ = Forge::GitLab.release(&cli(), &response);
            let _ = Forge::GitLab.repository(&response);
        }

        #[test]
        fn a_hook_s_time_reads_in_either_form(seconds in 0_i64..4_102_444_800) {
            let time = jiff::Timestamp::from_second(seconds).unwrap();
            let written = time.strftime("%Y-%m-%d %H:%M:%S UTC").to_string();
            for form in [written, time.to_string()] {
                let event = Forge::GitLab
                    .event(&edited(|payload| payload["released_at"] = json!(form)))
                    .unwrap()
                    .unwrap();
                prop_assert_eq!(event.occurred(), time);
            }
        }
    }
}

//! Forgejo's release event and releases API, read from what Codeberg sends.

#[cfg(test)]
mod forgejo {
    use herald_core::Kind;
    use herald_core::PrereleaseRule;
    use herald_ports::{Response, Secret};
    use herald_sources_forge::{Forge, Hooked, Origin, Repository, Signals, SourceOptions, Token};
    use herald_wire::SourceError;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    const EVENT: &str = include_str!("../fixtures/forgejo/release-event.reference.json");
    const RELEASES: &str = include_str!("../fixtures/forgejo/releases.json");
    const REPOSITORY: &str = include_str!("../fixtures/forgejo/repository.json");

    fn answer(status: u16, body: &str) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn forgejo() -> Repository {
        Forge::Forgejo.repository(&answer(200, REPOSITORY)).unwrap()
    }

    fn source(api: Option<&str>) -> Origin {
        Origin::new(
            Forge::Forgejo,
            &SourceOptions {
                api: api.map(ToOwned::to_owned),
                ..SourceOptions::default()
            },
            "forgejo/forgejo",
        )
    }

    #[test]
    fn a_webhook_named_by_either_of_forgejo_s_event_headers_announces_its_release() {
        let event = Forge::Forgejo
            .event(EVENT.as_bytes(), PrereleaseRule::default())
            .unwrap()
            .unwrap();
        for header in ["X-Forgejo-Event", "X-Gitea-Event"] {
            assert_eq!(
                Forge::Forgejo.hooked(
                    (&[(header, "release")], EVENT.as_bytes()),
                    "forgejo/forgejo",
                    &Signals::default()
                ),
                Ok(Hooked::Event(event.clone()))
            );
        }
        assert_eq!(
            Forge::Forgejo.hooked(
                (&[("X-Gitea-Event", "push")], EVENT.as_bytes()),
                "forgejo/forgejo",
                &Signals::default()
            ),
            Ok(Hooked::Nothing(
                "the delivery is not of a release, a tag or a package"
            ))
        );
        assert!(
            Forge::Forgejo
                .hooked(
                    (&[("X-Forgejo-Event", "release")], EVENT.as_bytes()),
                    "other/repo",
                    &Signals::default()
                )
                .is_err()
        );
    }

    #[test]
    fn an_owner_s_webhook_is_admitted_by_the_repository_attributes_it_names() {
        const RELEASED: (&str, &str) = ("X-Forgejo-Event", "release");
        let taking = |selection: &herald_core::Selection| {
            Forge::Forgejo.hooked_owned(
                (&[RELEASED], EVENT.as_bytes()),
                ("forgejo", false),
                (selection, &|_| Signals::default()),
            )
        };
        assert!(matches!(
            taking(&herald_core::Selection {
                topics: vec!["git".to_owned()],
                language: vec!["go".to_owned()],
                properties: std::collections::BTreeMap::from([(
                    "team".to_owned(),
                    vec!["media".to_owned()],
                )]),
                ..herald_core::Selection::default()
            }),
            Ok(Hooked::Member { ref id, .. }) if id == "73144"
        ));
        for refusing in [
            herald_core::Selection {
                topics: vec!["media".to_owned()],
                ..herald_core::Selection::default()
            },
            herald_core::Selection {
                language: vec!["Rust".to_owned()],
                ..herald_core::Selection::default()
            },
        ] {
            assert_eq!(
                taking(&refusing),
                Ok(Hooked::Nothing(
                    "the repository is not one the source takes"
                )),
                "{refusing:?}"
            );
        }
    }

    #[test]
    fn a_published_release_event_becomes_an_event() {
        let event = Forge::Forgejo
            .event(EVENT.as_bytes(), PrereleaseRule::default())
            .unwrap()
            .unwrap();
        assert_eq!(event.identity().as_str(), "codeberg.org/73144/v17.0.1");
        assert_eq!(event.kind(), &Kind::RELEASE);
        assert_eq!(event.project(), "forgejo");
        assert_eq!(event.title(), "v17.0.1");
        assert_eq!(
            event.link("release"),
            Some("https://codeberg.org/forgejo/forgejo/releases/tag/v17.0.1")
        );
        assert_eq!(event.occurred().to_string(), "2026-10-26T18:39:32Z");
        let mut updated: Value = serde_json::from_str(EVENT).unwrap();
        updated["action"] = json!("updated");
        let edited = Forge::Forgejo
            .event(updated.to_string().as_bytes(), PrereleaseRule::default())
            .unwrap()
            .unwrap();
        assert!(edited.edited() && !event.edited());
        assert_eq!(edited.identity(), event.identity());
        updated["action"] = json!("deleted");
        assert_eq!(
            Forge::Forgejo.event(updated.to_string().as_bytes(), PrereleaseRule::default()),
            Ok(None)
        );
    }

    #[test]
    fn a_poll_finds_the_published_releases_newest_first() {
        assert_eq!(
            forgejo(),
            Repository {
                id: 73_144,
                name: "forgejo".to_owned(),
                page: Some("https://codeberg.org/forgejo/forgejo".to_owned())
            }
        );
        let releases = Forge::Forgejo
            .releases(
                &forgejo(),
                &answer(200, RELEASES),
                (herald_wire::PARSE_LIMIT, PrereleaseRule::default()),
            )
            .unwrap();
        let found: Vec<&str> = releases
            .events
            .iter()
            .map(|event| event.identity().as_str())
            .collect();
        assert_eq!(
            found,
            [
                "codeberg.org/73144/v17.0.1",
                "codeberg.org/73144/v16.0.7",
                "codeberg.org/73144/v15.0.11",
            ]
        );
        assert_eq!(releases.etag, None);
        assert_eq!(
            Forge::Forgejo
                .releases(
                    &forgejo(),
                    &answer(500, "{}"),
                    (herald_wire::PARSE_LIMIT, PrereleaseRule::default())
                )
                .unwrap_err(),
            SourceError::Answered {
                what: "Forgejo's releases",
                status: 500
            }
        );
    }

    #[test]
    fn the_requests_carry_a_forgejo_token_and_ask_codeberg_where_no_api_is_named() {
        let named = Token::Named(Secret::new("CODEBERG_TOKEN", "hunter2"));
        assert_eq!(
            source(None)
                .releases_request(None, Some(&named))
                .to_string(),
            [
                "GET https://codeberg.org/api/v1/repos/forgejo/forgejo/releases?limit=30",
                "Accept: application/json",
                "User-Agent: herald/0.0.0 (+https://github.com/nightworksio/herald)",
                "Authorization: token «secret:CODEBERG_TOKEN»",
            ]
            .join("\n")
        );
        let run = Token::Run(Secret::new("GITHUB_TOKEN", "hunter2"));
        let own = source(Some("https://forge.example/api/v1"));
        assert_eq!(
            own.release_request("v17.0.1", Some(&run)).to_string(),
            [
                "GET https://forge.example/api/v1/repos/forgejo/forgejo/releases/tags/v17.0.1",
                "Accept: application/json",
                "User-Agent: herald/0.0.0 (+https://github.com/nightworksio/herald)",
                "Authorization: token «secret:GITHUB_TOKEN»",
            ]
            .join("\n")
        );
        let releases: Value = serde_json::from_str(RELEASES).unwrap();
        let event = Forge::Forgejo
            .release(
                &forgejo(),
                &answer(200, &releases[2].to_string()),
                PrereleaseRule::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.version(), Some("v15.0.11"));
    }

    proptest! {
        #[test]
        fn any_bytes_are_an_event_nothing_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = Forge::Forgejo.event(&bytes, PrereleaseRule::default());
            let response = Response { status: 200, headers: Vec::new(), body: bytes };
            let _ = Forge::Forgejo.releases(&forgejo(), &response, (herald_wire::PARSE_LIMIT, PrereleaseRule::default()));
            let _ = Forge::Forgejo.release(&forgejo(), &response, PrereleaseRule::default());
        }
    }
}

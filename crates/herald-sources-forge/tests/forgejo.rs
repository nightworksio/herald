//! Forgejo's release event and releases API, read from what Codeberg sends.

#[cfg(test)]
mod forgejo {
    use herald_core::Kind;
    use herald_ports::{Response, Secret};
    use herald_sources_forge::{Forge, ForgeError, Origin, Repository, SourceOptions, Token};
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
                repository: "forgejo/forgejo".to_owned(),
                api: api.map(ToOwned::to_owned),
                ..SourceOptions::default()
            },
        )
    }

    #[test]
    fn a_published_release_event_becomes_an_event() {
        let event = Forge::Forgejo.event(EVENT.as_bytes()).unwrap().unwrap();
        assert_eq!(event.identity().as_str(), "codeberg.org/73144/v17.0.1");
        assert_eq!(event.kind(), Kind::Release);
        assert_eq!(event.project(), "forgejo");
        assert_eq!(event.title(), "v17.0.1");
        assert_eq!(
            event.link("release"),
            Some("https://codeberg.org/forgejo/forgejo/releases/tag/v17.0.1")
        );
        assert_eq!(event.occurred().to_string(), "2026-10-26T18:39:32Z");
        let mut updated: Value = serde_json::from_str(EVENT).unwrap();
        updated["action"] = json!("updated");
        assert_eq!(
            Forge::Forgejo.event(updated.to_string().as_bytes()),
            Ok(None)
        );
    }

    #[test]
    fn a_poll_finds_the_published_releases_newest_first() {
        assert_eq!(
            forgejo(),
            Repository {
                id: 73_144,
                name: "forgejo".to_owned()
            }
        );
        let releases = Forge::Forgejo
            .releases(&forgejo(), &answer(200, RELEASES))
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
                .releases(&forgejo(), &answer(500, "{}"))
                .unwrap_err(),
            ForgeError::Answered {
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
            .release(&forgejo(), &answer(200, &releases[2].to_string()))
            .unwrap()
            .unwrap();
        assert_eq!(event.version(), Some("v15.0.11"));
    }

    proptest! {
        #[test]
        fn any_bytes_are_an_event_nothing_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = Forge::Forgejo.event(&bytes);
            let response = Response { status: 200, headers: Vec::new(), body: bytes };
            let _ = Forge::Forgejo.releases(&forgejo(), &response);
            let _ = Forge::Forgejo.release(&forgejo(), &response);
        }
    }
}

//! Packagist's, crates.io's, npm's and Docker Hub's answers, read from what
//! each registry sends.

#[cfg(test)]
mod registries {
    use herald_core::{Cursor, Kind, Problem};
    use herald_ports::{Response, Secret};
    use herald_sources_registry::{Package, Registry, RegistryOptions};
    use herald_wire::SourceError;
    use proptest::prelude::*;
    use serde_json::json;

    const PACKAGIST: &str = include_str!("../fixtures/packagist-monolog.json");
    const CRATES_IO: &str = include_str!("../fixtures/crates-io-jiff.json");
    const NPM: &str = include_str!("../fixtures/npm-left-pad.json");
    const DOCKER_HUB: &str = include_str!("../fixtures/docker-hub-caddy.json");

    const REGISTRIES: [Registry; 4] = [
        Registry::Packagist,
        Registry::CratesIo,
        Registry::Npm,
        Registry::DockerHub,
    ];

    fn package(registry: Registry, name: &str, api: Option<&str>) -> Package {
        Package::new(
            registry,
            &RegistryOptions {
                package: name.to_owned(),
                api: api.map(ToOwned::to_owned),
                ..RegistryOptions::default()
            },
        )
    }

    fn answer(status: u16, body: &str) -> Response {
        Response {
            status,
            headers: vec![
                ("ETag".to_owned(), "\"6abc0fc9-14ab6\"".to_owned()),
                (
                    "Last-Modified".to_owned(),
                    "Tue, 29 Sep 2026 19:21:45 GMT".to_owned(),
                ),
            ],
            body: body.as_bytes().to_vec(),
        }
    }

    fn identities(package: &Package, body: &str) -> Vec<String> {
        package
            .versions(&answer(200, body))
            .unwrap()
            .events
            .iter()
            .map(|event| event.identity().as_str().to_owned())
            .collect()
    }

    #[test]
    fn packagist_s_newest_tagged_versions_become_events_with_the_validators() {
        let monolog = package(Registry::Packagist, "monolog/monolog", None);
        let polled = monolog.versions(&answer(200, PACKAGIST)).unwrap();
        assert_eq!(polled.events.len(), 30);
        let newest = &polled.events[0];
        assert_eq!(
            newest.identity().as_str(),
            "repo.packagist.org/monolog/monolog/3.12.1"
        );
        assert_eq!(newest.kind(), Kind::PackageVersion);
        assert_eq!(newest.project(), "monolog/monolog");
        assert_eq!(newest.title(), "monolog/monolog 3.12.1");
        assert_eq!(newest.version(), Some("3.12.1"));
        assert_eq!(newest.body(), "");
        assert_eq!(
            newest.link("release"),
            Some("https://packagist.org/packages/monolog/monolog#3.12.1")
        );
        assert_eq!(newest.occurred().to_string(), "2026-09-29T19:21:12Z");
        assert!(!newest.prerelease());
        assert_eq!(polled.etag.as_deref(), Some("\"6abc0fc9-14ab6\""));
        assert_eq!(
            polled.last_modified.as_deref(),
            Some("Tue, 29 Sep 2026 19:21:45 GMT")
        );
        let times: Vec<_> = polled
            .events
            .iter()
            .map(herald_core::Event::occurred)
            .collect();
        assert!(times.windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn a_minified_entry_takes_the_time_before_it_and_an_unset_one_has_none() {
        let entries = json!([
            { "version": "2.0.0", "time": "2026-01-02T00:00:00+00:00" },
            { "version": "1.0.0" },
            { "version": "0.9.0", "time": "__unset" },
            { "version": "0.8.0", "time": "last spring" },
        ]);
        let minified = json!({ "minified": "composer/2.0", "packages": { "A/B": entries } });
        let ab = package(Registry::Packagist, "a/b", None);
        assert_eq!(
            identities(&ab, &minified.to_string()),
            [
                "repo.packagist.org/a/b/2.0.0",
                "repo.packagist.org/a/b/1.0.0"
            ]
        );
        let full = json!({ "packages": { "a/b": entries } });
        assert_eq!(
            identities(&ab, &full.to_string()),
            ["repo.packagist.org/a/b/2.0.0"]
        );
        let other = json!({ "packages": { "c/d": entries } });
        assert_eq!(identities(&ab, &other.to_string()), Vec::<String>::new());
    }

    #[test]
    fn crates_io_s_versions_leave_out_the_yanked() {
        let jiff = package(Registry::CratesIo, "jiff", None);
        let found = identities(&jiff, CRATES_IO);
        assert_eq!(
            found[..2],
            ["crates.io/jiff/0.2.38", "crates.io/jiff/0.2.37"]
        );
        assert!(!found.iter().any(|identity| identity.ends_with("/0.2.36")));
        let polled = jiff.versions(&answer(200, CRATES_IO)).unwrap();
        assert_eq!(
            polled.events[0].link("release"),
            Some("https://crates.io/crates/jiff/0.2.38")
        );
    }

    #[test]
    fn npm_s_published_versions_carry_their_times_and_an_unpublished_one_is_gone() {
        let pad = package(Registry::Npm, "left-pad", None);
        let found = identities(&pad, NPM);
        assert_eq!(found.len(), 15);
        assert_eq!(found[0], "registry.npmjs.org/left-pad/1.3.0");
        let polled = pad.versions(&answer(200, NPM)).unwrap();
        assert_eq!(
            polled.events[0].link("release"),
            Some("https://www.npmjs.com/package/left-pad/v/1.3.0")
        );
        let unpublished = json!({
            "versions": { "1.0.0": {} },
            "time": { "1.0.0": "2026-01-01T00:00:00Z", "2.0.0": "2026-02-01T00:00:00Z" },
        });
        assert_eq!(
            identities(&pad, &unpublished.to_string()),
            ["registry.npmjs.org/left-pad/1.0.0"]
        );
        assert_eq!(identities(&pad, "{}"), Vec::<String>::new());
        let listed = pad.versions(&answer(200, r#"{"versions":["1.0.0"]}"#));
        assert!(
            matches!(&listed, Err(SourceError::Unreadable { reason, .. }) if reason.starts_with("invalid type: sequence, expected an object of versions")),
            "{listed:?}"
        );
    }

    #[test]
    fn docker_hub_s_tags_are_an_official_image_s_versions_without_library() {
        let caddy = package(Registry::DockerHub, "caddy", None);
        assert_eq!(caddy.name(), "library/caddy");
        let polled = caddy.versions(&answer(200, DOCKER_HUB)).unwrap();
        let first = &polled.events[0];
        assert_eq!(
            first.identity().as_str(),
            "hub.docker.com/library/caddy/latest"
        );
        assert_eq!(first.project(), "caddy");
        assert_eq!(first.title(), "caddy latest");
        let release = polled
            .events
            .iter()
            .find(|event| event.version() == Some("2.11.7"))
            .unwrap();
        assert_eq!(
            release.link("release"),
            Some("https://hub.docker.com/_/caddy/tags?name=2.11.7")
        );
        let own = package(Registry::DockerHub, "nightworksio/herald", None);
        let tags = json!({ "results": [
            { "name": "0.1.0", "tag_last_pushed": "2026-10-01T00:00:00Z" },
            { "name": "0.0.9", "last_updated": "2026-09-01T00:00:00Z" },
            { "name": "untimed" },
        ]});
        let polled = own.versions(&answer(200, &tags.to_string())).unwrap();
        assert_eq!(polled.events.len(), 2);
        assert_eq!(polled.events[0].project(), "nightworksio/herald");
        assert_eq!(
            polled.events[0].link("release"),
            Some("https://hub.docker.com/r/nightworksio/herald/tags?name=0.1.0")
        );
    }

    #[test]
    fn each_registry_is_asked_for_its_newest_versions_conditionally() {
        let cursor = Cursor {
            seen: Vec::new(),
            etag: Some("\"a\"".to_owned()),
            last_modified: Some("Tue, 29 Sep 2026 19:21:45 GMT".to_owned()),
        };
        let token = Secret::new("NPM_TOKEN", "hunter2");
        let asked = |registry, name: &str| {
            package(registry, name, None)
                .versions_request(Some(&cursor), Some(&token))
                .to_string()
        };
        let head = |url: &str| {
            format!(
                "GET {url}\nAccept: application/json\nUser-Agent: herald/0.0.0 (+https://github.com/nightworksio/herald)"
            )
        };
        let tail = "If-None-Match: \"a\"\nIf-Modified-Since: Tue, 29 Sep 2026 19:21:45 GMT";
        assert_eq!(
            asked(Registry::Packagist, "monolog/monolog"),
            format!(
                "{}\n{tail}",
                head("https://repo.packagist.org/p2/monolog/monolog.json")
            )
        );
        assert_eq!(
            asked(Registry::CratesIo, "jiff"),
            format!(
                "{}\n{tail}",
                head("https://crates.io/api/v1/crates/jiff/versions?per_page=30&sort=date")
            )
        );
        assert_eq!(
            asked(Registry::Npm, "@types/node"),
            format!(
                "{}\nAuthorization: Bearer «secret:NPM_TOKEN»\n{tail}",
                head("https://registry.npmjs.org/@types%2Fnode")
            )
        );
        assert_eq!(
            asked(Registry::DockerHub, "caddy"),
            format!(
                "{}\n{tail}",
                head(
                    "https://hub.docker.com/v2/namespaces/library/repositories/caddy/tags?page_size=30&ordering=last_updated"
                )
            )
        );
        let plain = package(Registry::Npm, "left-pad", None).versions_request(None, None);
        assert_eq!(
            plain.to_string(),
            head("https://registry.npmjs.org/left-pad")
        );
    }

    #[test]
    fn a_mirror_is_asked_in_place_of_the_public_registry_and_links_nowhere() {
        let mirror = package(Registry::Npm, "left-pad", Some("https://npm.example/"));
        assert_eq!(mirror.api(), "https://npm.example");
        assert_eq!(mirror.registry(), Registry::Npm);
        let polled = mirror.versions(&answer(200, NPM)).unwrap();
        assert_eq!(
            polled.events[0].identity().as_str(),
            "npm.example/left-pad/1.3.0"
        );
        assert_eq!(polled.events[0].link("release"), None);
    }

    #[test]
    fn what_each_registry_is_and_takes_is_named() {
        let kinds: Vec<&str> = REGISTRIES.iter().map(|registry| registry.kind()).collect();
        assert_eq!(kinds, ["packagist", "crates-io", "npm", "docker-hub"]);
        let tokens: Vec<bool> = REGISTRIES
            .iter()
            .map(|registry| registry.takes_token())
            .collect();
        assert_eq!(tokens, [false, false, true, false]);
        let moving: Vec<bool> = REGISTRIES
            .iter()
            .map(|registry| registry.moving_tags())
            .collect();
        assert_eq!(moving, [false, false, false, true]);
    }

    #[test]
    fn an_unchanged_refused_or_unreadable_answer_says_so() {
        assert!(Registry::unchanged(&answer(304, "")));
        assert!(!Registry::unchanged(&answer(200, NPM)));
        let jiff = package(Registry::CratesIo, "jiff", None);
        assert_eq!(
            jiff.versions(&answer(404, "{}")),
            Err(SourceError::Answered {
                what: "crates.io's versions",
                status: 404
            })
        );
        assert!(matches!(
            jiff.versions(&answer(200, "[]")),
            Err(SourceError::Unreadable {
                what: "crates.io's versions",
                ..
            })
        ));
        let nameless = package(Registry::CratesIo, "", None);
        assert_eq!(
            nameless.versions(&answer(200, CRATES_IO)),
            Err(SourceError::Invalid {
                what: "crates.io's versions",
                problem: Problem::NoProject
            })
        );
        let huge = " ".repeat(4_194_305);
        assert_eq!(
            jiff.versions(&answer(200, &huge)),
            Err(SourceError::TooLarge {
                what: "crates.io's versions",
                limit: 4_194_304
            })
        );
        let prerelease = json!({ "versions": [
            { "num": "1.0.0-rc.1", "created_at": "2026-01-01T00:00:00Z", "yanked": false },
        ]});
        let polled = jiff
            .versions(&answer(200, &prerelease.to_string()))
            .unwrap();
        assert!(polled.events[0].prerelease());
    }

    proptest! {
        #[test]
        fn any_bytes_are_versions_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let response = Response { status: 200, headers: Vec::new(), body: bytes };
            for registry in REGISTRIES {
                let _ = package(registry, "a/b", None).versions(&response);
            }
        }

        #[test]
        fn the_identity_is_the_registry_the_package_and_the_version(version in "[0-9a-z.+-]{1,16}") {
            let body = json!({ "versions": { version.clone(): {} }, "time": { version.clone(): "2026-01-01T00:00:00Z" } });
            let pad = package(Registry::Npm, "left-pad", None);
            let found = identities(&pad, &body.to_string());
            prop_assert_eq!(found, vec![format!("registry.npmjs.org/left-pad/{version}")]);
        }
    }
}

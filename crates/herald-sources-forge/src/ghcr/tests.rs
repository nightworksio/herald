use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;

/// The round each poll here is made in.
const ROUND: jiff::Timestamp = jiff::Timestamp::UNIX_EPOCH;

fn options() -> GhcrOptions {
    GhcrOptions {
        owner: "nightworksio".to_owned(),
        token: SecretRef {
            secret: "GITHUB_TOKEN".to_owned(),
        },
        api: None,
        registry: None,
        include: Vec::new(),
        exclude: Vec::new(),
        visibility: None,
        moving: None,
        package_visibility: None,
        most_packages: None,
        keep_members: None,
        project: None,
        backfill: 0,
        page: None,
        remembered: None,
        parse: None,
    }
}

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: vec![("ETag".to_owned(), "\"e\"".to_owned())],
        body: body.to_string().into_bytes(),
    }
}

fn images(options: &GhcrOptions) -> Images {
    Images::new(
        options,
        Some(Token::Named(herald_ports::Secret::new("GITHUB_TOKEN", "t"))),
    )
}

fn image(id: u64, name: &str, visibility: &str, linked: Option<&str>) -> Value {
    json!({
        "id": id,
        "name": name,
        "visibility": visibility,
        "repository": linked.map(|repository| json!({
            "id": 9,
            "name": repository,
            "full_name": format!("nightworksio/{repository}"),
            "visibility": "public",
            "topics": ["rust"]
        }))
    })
}

fn listed(source: &Images, http: &ScriptedHttp) -> Result<Expanded, Unlisted> {
    pollster::block_on(source.listed(http, None))
}

fn members(expanded: Expanded) -> Vec<Listed> {
    expanded.members()
}

#[test]
fn a_ghcr_source_takes_an_owner_and_a_token_and_checks_each_key() {
    assert_eq!(options().check(), Ok(()));
    let cases: [(GhcrOptions, &str); 6] = [
        (
            GhcrOptions {
                owner: "a/b".to_owned(),
                ..options()
            },
            "\"a/b\" is not an owner; give the organization's or user's name, of letters, digits, '.', '_' and '-'",
        ),
        (
            GhcrOptions {
                api: Some("ghe".to_owned()),
                ..options()
            },
            "\"ghe\" is not a web address; give GitHub's REST API, such as https://ghe.example/api/v3",
        ),
        (
            GhcrOptions {
                registry: Some("https://x".to_owned()),
                ..options()
            },
            "registry \"https://x\" is not a registry's host; give it as registry.example or registry.example:5000",
        ),
        (
            GhcrOptions {
                most_packages: NonZeroUsize::new(MOST_PACKAGES_ALLOWED + 1),
                ..options()
            },
            "most_packages is at most 100000; lower it",
        ),
        (
            GhcrOptions {
                project: Some("{{ event.version }}".to_owned()),
                ..options()
            },
            "project reads event.version, which a repository does not carry; use repository.id, repository.name, repository.path, repository.owner, repository.topics, repository.properties, repository.visibility",
        ),
        (
            GhcrOptions {
                page: NonZeroUsize::new(600),
                ..options()
            },
            "the cursor remembers 500 identities, fewer than the 600 one poll reads; make remembered at least page",
        ),
    ];
    for (given, error) in cases {
        assert_eq!(given.check(), Err(error.to_owned()));
    }
    for bad in [
        GhcrOptions {
            include: vec!["[".to_owned()],
            ..options()
        },
        GhcrOptions {
            moving: Some(vec!["[".to_owned()]),
            ..options()
        },
    ] {
        assert!(bad.check().is_err());
    }
    assert_eq!(
        GhcrOptions {
            most_packages: NonZeroUsize::new(MOST_PACKAGES_ALLOWED),
            registry: Some("registry.example:5000".to_owned()),
            api: Some("https://ghe.example/api/v3/".to_owned()),
            ..options()
        }
        .check(),
        Ok(())
    );
}

#[test]
fn a_ghcr_source_names_its_registry_beside_its_api_and_is_a_kind_of_its_own() {
    assert_eq!(options().api(), "https://api.github.com");
    assert_eq!(options().registry(), "ghcr.io");
    let enterprise = GhcrOptions {
        api: Some("https://ghe.example/api/v3/".to_owned()),
        ..options()
    };
    assert_eq!(enterprise.api(), "https://ghe.example/api/v3");
    assert_eq!(enterprise.registry(), "containers.ghe.example");
    assert_eq!(
        GhcrOptions {
            registry: Some("registry.example".to_owned()),
            ..options()
        }
        .registry(),
        "registry.example"
    );
    assert_eq!(GhcrKind::NAME, "ghcr");
    let source = GhcrKind::checked(options()).unwrap();
    assert_eq!(source.kind(), "ghcr");
    assert_eq!(source.token(), Some(&options().token));
    assert_eq!(source.kinds(), &[Kind::PACKAGE_VERSION]);
    assert_eq!(source.backfill(), 0);
    assert!(!source.unlisted());
    assert!(
        GhcrKind::checked(GhcrOptions {
            visibility: Some(vec![Visibility::Private]),
            ..options()
        })
        .unwrap()
        .unlisted()
    );
    assert_eq!(
        GhcrKind::checked(GhcrOptions {
            owner: String::new(),
            ..options()
        })
        .unwrap_err(),
        Fault::here(
            "\"\" is not an owner; give the organization's or user's name, of letters, digits, '.', '_' and '-'"
        )
    );
    let Polling::Many(expand) = source.poller(None, false) else {
        panic!("an owner's images are polled as members");
    };
    assert_eq!(expand.api(), "https://api.github.com");
    assert_eq!(
        expand.member_nouns(),
        ("container image", "container images")
    );
    assert!(!expand.pushed());
    assert_eq!(expand.keep_members(), KEEP_MEMBERS);
    assert_eq!(expand.remembered(), 500);
    let kept = Images::new(
        &GhcrOptions {
            keep_members: Some(Duration(SignedDuration::from_hours(2))),
            ..options()
        },
        None,
    );
    assert_eq!(kept.keep_members(), SignedDuration::from_hours(2));
}

#[test]
fn an_owner_s_images_are_listed_page_by_page_an_organization_first_then_a_user() {
    let source = images(&GhcrOptions {
        exclude: vec!["old".to_owned()],
        ..options()
    });
    let http = ScriptedHttp::new(vec![
        answer(404, &json!({})),
        Response {
            headers: vec![(
                "Link".to_owned(),
                "<https://api.github.com/x?page=2>; rel=\"next\"".to_owned(),
            )],
            ..answer(
                200,
                &json!([
                    image(1, "herald", "public", Some("herald")),
                    image(2, "old", "public", None)
                ]),
            )
        },
        answer(
            200,
            &json!([
                image(3, "lone", "public", None),
                image(4, "secret", "private", None)
            ]),
        ),
    ]);
    let expanded = listed(&source, &http).unwrap();
    let Expanded::Listed { owner, pages, .. } = &expanded else {
        panic!("listed");
    };
    assert_eq!((*owner, pages.len()), (Owner::User, 2));
    let taken = members(expanded);
    let names: Vec<&str> = taken.iter().map(|image| image.name.as_str()).collect();
    assert_eq!(names, ["herald", "lone"]);
    assert_eq!(taken[0].id, "1");
    assert_eq!(
        taken[0].linked.as_ref().unwrap().path,
        "nightworksio/herald"
    );
    assert_eq!(taken[1].linked, None);
    let urls: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        urls,
        [
            "https://api.github.com/orgs/nightworksio/packages?package_type=container&per_page=100&page=1",
            "https://api.github.com/users/nightworksio/packages?package_type=container&per_page=100&page=1",
            "https://api.github.com/users/nightworksio/packages?package_type=container&per_page=100&page=2",
        ]
    );
    assert_eq!(
        http.requests()[0]
            .headers
            .iter()
            .find(|(name, _)| name == "Authorization")
            .map(|(_, value)| value.redacted()),
        Some("Bearer «secret:GITHUB_TOKEN»".to_owned())
    );
}

#[test]
fn a_listing_over_most_packages_or_one_that_fails_lists_nothing() {
    let source = images(&GhcrOptions {
        most_packages: NonZeroUsize::new(1),
        ..options()
    });
    let two = json!([image(1, "a", "public", None), image(2, "b", "public", None)]);
    assert_eq!(
        listed(&source, &ScriptedHttp::new(vec![answer(200, &two)])).unwrap_err(),
        Unlisted::Over(
            "nightworksio lists more than 1 container images, most_packages; raise it, or narrow include and exclude"
                .to_owned()
        )
    );
    let source = images(&options());
    assert_eq!(
        listed(&source, &ScriptedHttp::new(vec![answer(500, &json!({}))])).unwrap_err(),
        failed(SourceError::Answered {
            what: Forge::GitHub.api().packages,
            status: 500
        })
    );
    assert!(listed(&source, &ScriptedHttp::new(vec![answer(200, &json!({}))])).is_err());
    assert!(matches!(
        listed(&source, &ScriptedHttp::new(Vec::new())),
        Err(Unlisted::Failed(Unpolled::Unreachable(_)))
    ));
    let known = Listing {
        owner: Owner::User,
        pages: Vec::new(),
        first: Timestamp::UNIX_EPOCH,
        listed: Timestamp::UNIX_EPOCH,
        fingerprint: source.fingerprint(),
        file: None,
        stale: false,
        events: None,
    };
    let http = ScriptedHttp::new(vec![answer(404, &json!({}))]);
    assert!(pollster::block_on(source.listed(&http, Some(&known))).is_err());
    assert_eq!(
        http.asked()[0].0,
        "https://api.github.com/users/nightworksio/packages?package_type=container&per_page=100&page=1"
    );
}

fn versions() -> Value {
    json!([
        {"html_url": "https://github.com/v2", "created_at": "2026-01-02T00:00:00Z",
         "metadata": {"container": {"tags": ["v2.0.0-rc.1"]}}},
        {"html_url": "https://github.com/v1", "created_at": "2026-01-01T00:00:00Z",
         "metadata": {"container": {"tags": ["v1.0.0", "latest"]}}},
        {"created_at": "2025-12-31T00:00:00Z", "metadata": {}}
    ])
}

fn polled(
    poll: &Arc<dyn Poll>,
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
) -> Result<Option<Polled>, Unpolled> {
    pollster::block_on(poll.polled(http, cursor, ROUND))
}

#[test]
fn each_image_s_tags_are_announced_under_the_registry_s_identity_as_its_repository_s_project() {
    let source = images(&GhcrOptions {
        project: Some("{{ repository.path }}".to_owned()),
        visibility: Some(vec![Visibility::Public, Visibility::Private]),
        ..options()
    });
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            image(1, "Herald", "private", Some("herald")),
            image(2, "lone", "public", None)
        ]),
    )]);
    let taken = listed(&source, &http).unwrap().members();
    assert_eq!(
        source.project(&taken[0]),
        Ok("nightworksio/herald".to_owned())
    );
    assert_eq!(source.project(&taken[1]), Ok("lone".to_owned()));
    assert_eq!(
        images(&options()).project(&taken[0]),
        Ok("herald".to_owned())
    );
    let broken = Images {
        project: ProjectName::compile("{{ repository.properties.team[0] }}", Sandbox::DEFAULT).ok(),
        ..images(&options())
    };
    assert!(broken.project(&taken[0]).is_err());
    assert!(source.takes(&taken[1]));
    let poll = source.member(&taken[0]);
    assert_eq!(poll.api(), "https://api.github.com");
    assert_eq!(poll.remembered(), 500);
    let http = ScriptedHttp::new(vec![answer(200, &versions())]);
    let found = polled(&poll, &http, None).unwrap().unwrap();
    let identities: Vec<&str> = found
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect();
    assert_eq!(
        identities,
        [
            "ghcr.io/nightworksio/herald/v2.0.0-rc.1",
            "ghcr.io/nightworksio/herald/v1.0.0"
        ]
    );
    let newest = &found.events[0];
    assert_eq!(newest.project(), "nightworksio/herald");
    assert_eq!(newest.title(), "Herald v2.0.0-rc.1");
    assert!(newest.prerelease());
    assert_eq!(newest.link("release"), Some("https://github.com/v2"));
    assert_eq!(newest.visibility(), Some(Visibility::Private));
    assert_eq!(found.etag.as_deref(), Some("\"e\""));
    assert_eq!(
        http.asked()[0].0,
        "https://api.github.com/orgs/nightworksio/packages/container/Herald/versions?per_page=30"
    );
    let lone = source.member(&taken[1]);
    let http = ScriptedHttp::new(vec![answer(200, &versions())]);
    let found = polled(&lone, &http, None).unwrap().unwrap();
    assert_eq!(found.events[0].project(), "lone");
    assert_eq!(
        found.events[1].link("release"),
        Some("https://github.com/v1")
    );
}

#[test]
fn an_image_polled_before_its_owner_is_listed_asks_an_organization_then_a_user() {
    let source = images(&options());
    let member = Images::member_of(
        serde_json::from_value(image(1, "a", "public", None)).unwrap(),
        PackageVisibility::Stricter,
    );
    let poll = source.member(&member);
    let http = ScriptedHttp::new(vec![answer(404, &json!({})), answer(304, &json!({}))]);
    let cursor = Cursor {
        etag: Some("\"e\"".to_owned()),
        ..Cursor::default()
    };
    assert_eq!(polled(&poll, &http, Some(&cursor)), Ok(None));
    let asked = http.asked();
    assert_eq!(
        asked[1],
        (
            "https://api.github.com/users/nightworksio/packages/container/a/versions?per_page=30"
                .to_owned(),
            Some("\"e\"".to_owned())
        )
    );
    let http = ScriptedHttp::new(vec![answer(404, &json!({})), answer(404, &json!({}))]);
    assert!(matches!(
        polled(&poll, &http, None),
        Err(Unpolled::Unreadable(_))
    ));
    assert!(matches!(
        polled(&poll, &ScriptedHttp::new(Vec::new()), None),
        Err(Unpolled::Unreachable(_))
    ));
    let http = ScriptedHttp::new(vec![answer(404, &json!({}))]);
    assert!(matches!(
        polled(&poll, &http, None),
        Err(Unpolled::Unreachable(_))
    ));
    let http = ScriptedHttp::new(vec![answer(200, &json!([{"created_at": "x"}]))]);
    assert!(matches!(
        polled(&poll, &http, None),
        Err(Unpolled::Unreadable(_))
    ));
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([{"html_url": "not a web address", "created_at": "2026-01-01T00:00:00Z",
                 "metadata": {"container": {"tags": ["v1"]}}}]),
    )]);
    assert!(matches!(
        polled(&poll, &http, None),
        Err(Unpolled::Unreadable(_))
    ));
    let user = ScriptedHttp::new(vec![answer(404, &json!({})), answer(200, &json!([]))]);
    listed(&source, &user).unwrap();
    let http = ScriptedHttp::new(vec![answer(200, &json!([]))]);
    assert_eq!(
        polled(&source.member(&member), &http, None)
            .unwrap()
            .unwrap()
            .events
            .len(),
        0
    );
    assert_eq!(
        http.asked()[0].0,
        "https://api.github.com/users/nightworksio/packages/container/a/versions?per_page=30"
    );
}

#[test]
fn a_ghcr_source_s_fingerprint_follows_what_its_listing_asks_for() {
    let print = |options: &GhcrOptions| images(options).fingerprint();
    assert_eq!(print(&options()), print(&options()));
    for changed in [
        GhcrOptions {
            owner: "other".to_owned(),
            ..options()
        },
        GhcrOptions {
            api: Some("https://ghe.example/api/v3".to_owned()),
            ..options()
        },
        GhcrOptions {
            include: vec!["a".to_owned()],
            ..options()
        },
    ] {
        assert_ne!(print(&changed), print(&options()));
    }
}

#[test]
fn a_public_image_of_a_private_repository_is_private_unless_the_image_decides() {
    let private = |seen: Option<PackageVisibility>| {
        let mut linked = image(1, "a", "public", Some("secret"));
        linked["repository"]["visibility"] = json!("private");
        let source = images(&GhcrOptions {
            package_visibility: seen,
            visibility: Some(vec![Visibility::Public, Visibility::Private]),
            ..options()
        });
        let http = ScriptedHttp::new(vec![answer(200, &json!([linked]))]);
        listed(&source, &http).unwrap().members()[0].visibility
    };
    assert_eq!(private(None), Visibility::Private);
    assert_eq!(
        private(Some(PackageVisibility::Stricter)),
        Visibility::Private
    );
    assert_eq!(
        private(Some(PackageVisibility::Package)),
        Visibility::Public
    );
    let mut linked = image(1, "a", "public", Some("secret"));
    linked["repository"]["visibility"] = json!("private");
    let public_only = images(&options());
    let http = ScriptedHttp::new(vec![answer(200, &json!([linked]))]);
    assert_eq!(listed(&public_only, &http).unwrap().members().len(), 0);
    let lone = Images::member_of(
        serde_json::from_value(image(2, "b", "public", None)).unwrap(),
        PackageVisibility::Stricter,
    );
    assert_eq!(lone.visibility, Visibility::Public);
    assert_ne!(
        images(&GhcrOptions {
            package_visibility: Some(PackageVisibility::Package),
            ..options()
        })
        .fingerprint(),
        images(&options()).fingerprint()
    );
}

#[test]
fn a_linked_image_stands_for_its_repository_and_an_unlinked_one_for_none() {
    let source = images(&GhcrOptions {
        visibility: Some(vec![Visibility::Public, Visibility::Private]),
        ..options()
    });
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            image(1, "a", "public", Some("herald")),
            image(2, "b", "public", None)
        ]),
    )]);
    let taken = listed(&source, &http).unwrap().members();
    assert_eq!(
        source.standing(&taken[0]),
        Some(herald_kinds::Standing::new(
            "https://api.github.com",
            "nightworksio/herald",
            true
        ))
    );
    assert_eq!(source.standing(&taken[1]), None);
    let origin = source.origin(&taken[0]);
    assert_eq!(
        (
            origin.forge.as_str(),
            origin.host.as_str(),
            origin.owner.as_str(),
            origin.repository.as_str(),
            origin.visibility
        ),
        (
            "ghcr",
            "ghcr.io",
            "nightworksio",
            "nightworksio/a",
            Visibility::Public
        )
    );
    assert_eq!(origin.topics, ["rust"]);
    assert_eq!(source.origin(&taken[1]).topics, Vec::<String>::new());
}

#[test]
fn a_ghcr_source_s_settings_and_answers_reach_what_it_announces() {
    let source = GhcrKind::checked(GhcrOptions {
        backfill: 3,
        ..options()
    })
    .unwrap();
    assert_eq!(source.backfill(), 3);
    let two = json!([image(1, "a", "public", None), image(2, "b", "public", None)]);
    let exact = images(&GhcrOptions {
        most_packages: NonZeroUsize::new(2),
        exclude: vec!["b".to_owned()],
        visibility: Some(vec![Visibility::Public, Visibility::Private]),
        ..options()
    });
    let taken = listed(&exact, &ScriptedHttp::new(vec![answer(200, &two)]))
        .unwrap()
        .members();
    assert_eq!(taken.len(), 1);
    let b = Images::member_of(
        serde_json::from_value(image(2, "b", "public", None)).unwrap(),
        PackageVisibility::Stricter,
    );
    assert!(!exact.takes(&b));
    let mut linked = image(3, "c", "private", Some("herald"));
    linked["repository"]["custom_properties"] = json!({"team": "core"});
    let private = Images::member_of(
        serde_json::from_value(linked).unwrap(),
        PackageVisibility::Stricter,
    );
    let origin = exact.origin(&private);
    assert_eq!(origin.visibility, Visibility::Private);
    assert_eq!(
        origin.properties.get("team").map(Vec::as_slice),
        Some(["core".to_owned()].as_slice())
    );
    let http = ScriptedHttp::new(vec![Response {
        headers: vec![
            ("ETag".to_owned(), "\"e\"".to_owned()),
            (
                "Last-Modified".to_owned(),
                "Tue, 29 Sep 2026 19:21:45 GMT".to_owned(),
            ),
        ],
        ..answer(200, &versions())
    }]);
    let found = polled(&exact.member(&taken[0]), &http, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        found.last_modified.as_deref(),
        Some("Tue, 29 Sep 2026 19:21:45 GMT")
    );
}

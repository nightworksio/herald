use std::num::NonZeroUsize;

use herald_core::SecretRef;
use herald_testing::ScriptedHttp;
use jiff::Timestamp;
use serde_json::{Value, json};

use super::*;
use crate::{Declared, VendorListing};

/// The round each poll here is made in.
const ROUND: jiff::Timestamp = jiff::Timestamp::UNIX_EPOCH;

fn answer(status: u16, body: &Value, etag: Option<&str>) -> Response {
    Response {
        status,
        headers: etag
            .map(|etag| vec![("ETag".to_owned(), etag.to_owned())])
            .unwrap_or_default(),
        body: body.to_string().into_bytes(),
    }
}

fn token() -> SecretRef {
    SecretRef {
        secret: "TOKEN".to_owned(),
    }
}

fn namespaced(registry: Registry, options: &RegistryOptions, token: Option<&str>) -> Namespaced {
    let namespace = options.namespace_of(registry).unwrap().to_owned();
    Namespaced::new(
        registry,
        &namespace,
        options,
        token.map(|token| Secret::new("TOKEN", token)),
    )
}

fn listed(
    source: &Namespaced,
    http: &ScriptedHttp,
    known: Option<&Listing>,
) -> Result<Expanded, Unlisted> {
    pollster::block_on(source.listed(http, known))
}

fn members(expanded: &Expanded) -> Vec<(&str, &str, Visibility)> {
    match expanded {
        Expanded::Listed { members, .. } => members
            .iter()
            .map(|member| {
                (
                    member.path.as_str(),
                    member.name.as_str(),
                    member.visibility,
                )
            })
            .collect(),
        Expanded::Unchanged => Vec::new(),
    }
}

fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(header, _)| header == name)
        .map(|(_, value)| value.redacted())
}

fn known(source: &Namespaced, pages: Vec<Option<String>>) -> Listing {
    Listing {
        owner: Owner::Organization,
        pages,
        first: Timestamp::UNIX_EPOCH,
        listed: Timestamp::UNIX_EPOCH,
        fingerprint: source.fingerprint(),
        file: None,
        stale: false,
        events: None,
    }
}

fn scope(unscoped: bool) -> RegistryOptions {
    RegistryOptions {
        scope: Some("@acme".to_owned()),
        unscoped,
        ..RegistryOptions::default()
    }
}

fn org() -> Value {
    json!({"@acme/a": "write", "@acme/b": "read", "@other/c": "write", "plain": "write"})
}

#[test]
fn an_npm_scope_lists_its_organization_s_scoped_packages_with_its_token() {
    let source = namespaced(Registry::Npm, &scope(false), Some("t"));
    let http = ScriptedHttp::new(vec![answer(200, &org(), Some("\"o1\""))]);
    let expanded = listed(&source, &http, None).unwrap();
    assert_eq!(
        members(&expanded),
        [
            ("@acme/a", "a", Visibility::Public),
            ("@acme/b", "b", Visibility::Public)
        ]
    );
    let Expanded::Listed {
        owner,
        pages,
        file,
        missing,
        ..
    } = expanded
    else {
        panic!("listed");
    };
    assert_eq!(
        (owner, pages, file, missing),
        (
            Owner::Organization,
            vec![Some("\"o1\"".to_owned())],
            None,
            Vec::new()
        )
    );
    let requests = http.requests();
    assert_eq!(
        requests[0].url.expose(),
        "https://registry.npmjs.org/-/org/acme/package"
    );
    assert_eq!(
        header(&requests[0], "Authorization"),
        Some("Bearer «secret:TOKEN»".to_owned())
    );
    assert_eq!(header(&requests[0], "Accept"), Some(media::JSON.to_owned()));
    let unscoped = namespaced(Registry::Npm, &scope(true), None);
    let http = ScriptedHttp::new(vec![answer(200, &org(), None)]);
    assert_eq!(
        members(&listed(&unscoped, &http, None).unwrap()),
        [
            ("@acme/a", "a", Visibility::Public),
            ("@acme/b", "b", Visibility::Public),
            ("plain", "plain", Visibility::Public)
        ]
    );
    assert_eq!(header(&http.requests()[0], "Authorization"), None);
}

#[test]
fn a_listing_in_one_answer_is_asked_again_with_its_etag_and_may_be_unchanged() {
    let source = namespaced(Registry::Npm, &scope(false), None);
    let last = known(&source, vec![Some("\"o1\"".to_owned())]);
    let http = ScriptedHttp::new(vec![answer(304, &json!({}), None)]);
    assert_eq!(
        listed(&source, &http, Some(&last)).unwrap(),
        Expanded::Unchanged
    );
    assert_eq!(
        http.asked()[0].1.as_deref(),
        Some("\"o1\""),
        "asked conditionally"
    );
    let changed = ScriptedHttp::new(vec![answer(200, &org(), Some("\"o2\""))]);
    assert_eq!(
        members(&listed(&source, &changed, Some(&last)).unwrap()).len(),
        2,
        "a conditional listing answered in full is read"
    );
    for stale in [
        Listing {
            fingerprint: "another".to_owned(),
            ..last.clone()
        },
        Listing {
            pages: vec![None],
            ..last.clone()
        },
        Listing {
            pages: vec![Some("\"a\"".to_owned()), Some("\"b\"".to_owned())],
            ..last.clone()
        },
    ] {
        let http = ScriptedHttp::new(vec![answer(200, &org(), None)]);
        assert_eq!(
            members(&listed(&source, &http, Some(&stale)).unwrap()).len(),
            2
        );
        assert_eq!(http.asked()[0].1, None);
    }
    let hub = namespaced(Registry::DockerHub, &hub(None), None);
    let paged = known(&hub, vec![Some("\"h\"".to_owned())]);
    let http = ScriptedHttp::new(vec![answer(
        200,
        &repositories(&[("a", false)], false),
        None,
    )]);
    assert_eq!(
        members(&listed(&hub, &http, Some(&paged)).unwrap()).len(),
        1
    );
    assert_eq!(http.asked()[0].1, None, "a paged listing is asked whole");
}

#[test]
fn npm_s_refusal_to_list_an_organization_says_how_to_list_it_and_other_failures_say_theirs() {
    let source = namespaced(Registry::Npm, &scope(false), None);
    for status in [401, 403, 404] {
        let http = ScriptedHttp::new(vec![answer(status, &json!({}), None)]);
        assert_eq!(
            listed(&source, &http, None).unwrap_err(),
            Unlisted::Failed(Unpolled::Unreadable(format!(
                "npm answered {status} to the listing of the organization @acme; give a token that may read it, or name its packages one by one with package"
            )))
        );
    }
    let http = ScriptedHttp::new(vec![answer(500, &json!({}), None)]);
    assert_eq!(
        listed(&source, &http, None).unwrap_err(),
        failed(SourceError::Answered {
            what: npm::ORG,
            status: 500
        })
    );
    let http = ScriptedHttp::new(vec![answer(200, &json!([]), None)]);
    assert!(matches!(
        listed(&source, &http, None),
        Err(Unlisted::Failed(Unpolled::Unreadable(_)))
    ));
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        listed(&source, &http, None),
        Err(Unlisted::Failed(Unpolled::Unreachable(_)))
    ));
}

fn vendor(listing: Option<VendorListing>, api: Option<&str>) -> RegistryOptions {
    RegistryOptions {
        vendor: Some("acme".to_owned()),
        listing,
        listing_api: api.map(ToOwned::to_owned),
        token: Some(token()),
        ..RegistryOptions::default()
    }
}

#[test]
fn a_packagist_vendor_is_listed_at_its_listing_api_without_its_token() {
    let source = namespaced(Registry::Packagist, &vendor(None, None), Some("t"));
    assert_eq!(
        source.bases(),
        ["https://repo.packagist.org", "https://packagist.org"]
    );
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!({"packageNames": ["acme/a", "acme/b"]}),
        None,
    )]);
    assert_eq!(
        members(&listed(&source, &http, None).unwrap()),
        [
            ("acme/a", "a", Visibility::Private),
            ("acme/b", "b", Visibility::Private)
        ]
    );
    let request = &http.requests()[0];
    assert_eq!(
        request.url.expose(),
        "https://packagist.org/packages/list.json?vendor=acme"
    );
    assert_eq!(header(request, "Authorization"), None);
    let mirrored = namespaced(
        Registry::Packagist,
        &vendor(None, Some("https://packagist.example/")),
        None,
    );
    assert_eq!(mirrored.bases()[1], "https://packagist.example");
}

#[test]
fn a_composer_repository_lists_a_vendor_s_available_packages_with_its_token() {
    let options = RegistryOptions {
        api: Some("https://repo.packagist.com/acme/".to_owned()),
        ..vendor(Some(VendorListing::Composer), None)
    };
    let source = namespaced(Registry::Packagist, &options, Some("t"));
    assert_eq!(source.bases(), &[] as &[String]);
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!({"available-packages": ["Acme/A", "other/b"]}),
        None,
    )]);
    assert_eq!(
        members(&listed(&source, &http, None).unwrap()),
        [("Acme/A", "A", Visibility::Private)]
    );
    let request = &http.requests()[0];
    assert_eq!(
        request.url.expose(),
        "https://repo.packagist.com/acme/packages.json"
    );
    assert_eq!(
        header(request, "Authorization"),
        Some("Basic «secret:TOKEN»".to_owned())
    );
    let http = ScriptedHttp::new(vec![answer(200, &json!({"packages": {}}), None)]);
    assert_eq!(
        listed(&source, &http, None).unwrap_err(),
        failed(SourceError::Unreadable {
            what: packagist::VENDOR,
            reason: "the Composer repository names no available-packages; name its packages one by one with package".to_owned()
        })
    );
}

fn owner(owner: &str) -> RegistryOptions {
    RegistryOptions {
        owner: Some(owner.to_owned()),
        listed_page: NonZeroUsize::new(2),
        ..RegistryOptions::default()
    }
}

fn crates(names: &[&str], next: bool) -> Value {
    json!({
        "crates": names.iter().map(|name| json!({"id": name, "name": name})).collect::<Vec<_>>(),
        "meta": {"next_page": next.then_some("?page=2")}
    })
}

#[test]
fn a_crates_io_owner_is_looked_up_then_its_crates_listed_page_by_page() {
    let source = namespaced(Registry::CratesIo, &owner("user:BurntSushi"), None);
    let http = ScriptedHttp::new(vec![
        answer(200, &json!({"user": {"id": 189}}), None),
        answer(
            200,
            &crates(&["aho-corasick", "bstr"], true),
            Some("\"p1\""),
        ),
        answer(200, &crates(&["csv"], false), None),
    ]);
    assert_eq!(
        members(&listed(&source, &http, None).unwrap()),
        [
            ("aho-corasick", "aho-corasick", Visibility::Public),
            ("bstr", "bstr", Visibility::Public),
            ("csv", "csv", Visibility::Public)
        ]
    );
    let urls: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        urls,
        [
            "https://crates.io/api/v1/users/BurntSushi",
            "https://crates.io/api/v1/crates?user_id=189&per_page=2&page=1",
            "https://crates.io/api/v1/crates?user_id=189&per_page=2&page=2",
        ]
    );
    let member = source.member_of("bstr".to_owned(), None);
    assert_eq!(source.origin(&member).owner, "user:BurntSushi");
    let team = namespaced(
        Registry::CratesIo,
        &owner("team:github:rust-lang:libs"),
        None,
    );
    let http = ScriptedHttp::new(vec![
        answer(200, &json!({"team": {"id": 11}}), None),
        answer(200, &crates(&[], true), None),
    ]);
    assert_eq!(members(&listed(&team, &http, None).unwrap()), []);
    let urls: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        urls,
        [
            "https://crates.io/api/v1/teams/github%3Arust-lang%3Alibs",
            "https://crates.io/api/v1/crates?team_id=11&per_page=2&page=1",
        ]
    );
}

#[test]
fn a_crates_io_owner_crates_io_does_not_name_lists_nothing() {
    let source = namespaced(Registry::CratesIo, &owner("user:nobody"), None);
    let http = ScriptedHttp::new(vec![answer(404, &json!({}), None)]);
    assert_eq!(
        listed(&source, &http, None).unwrap_err(),
        failed(SourceError::Answered {
            what: crates_io::OWNER,
            status: 404
        })
    );
    let http = ScriptedHttp::new(vec![answer(200, &json!({}), None)]);
    assert_eq!(
        listed(&source, &http, None).unwrap_err(),
        failed(SourceError::Unreadable {
            what: crates_io::OWNER,
            reason: "it names no user or team".to_owned()
        })
    );
    let http = ScriptedHttp::new(vec![answer(200, &json!([]), None)]);
    assert!(listed(&source, &http, None).is_err());
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        listed(&source, &http, None),
        Err(Unlisted::Failed(Unpolled::Unreachable(_)))
    ));
}

fn hub(token: Option<&str>) -> RegistryOptions {
    RegistryOptions {
        namespace: Some("acme".to_owned()),
        username: token.map(|_| "wessel".to_owned()),
        token: token.map(|_| self::token()),
        listed_page: NonZeroUsize::new(2),
        ..RegistryOptions::default()
    }
}

fn repositories(names: &[(&str, bool)], next: bool) -> Value {
    json!({
        "results": names.iter().map(|(name, private)| json!({"name": name, "namespace": "acme", "is_private": private})).collect::<Vec<_>>(),
        "next": next.then_some("https://hub.docker.com/v2/namespaces/acme/repositories?page=2")
    })
}

#[test]
fn a_docker_hub_namespace_is_listed_with_its_exchanged_token_and_each_repository_s_visibility() {
    let source = namespaced(Registry::DockerHub, &hub(Some("dckr")), Some("dckr"));
    let http = ScriptedHttp::new(vec![
        answer(200, &json!({"access_token": "short"}), None),
        answer(200, &repositories(&[("a", false), ("b", true)], true), None),
        answer(200, &repositories(&[("c", false)], false), None),
    ]);
    assert_eq!(
        members(&listed(&source, &http, None).unwrap()),
        [
            ("acme/a", "a", Visibility::Public),
            ("acme/b", "b", Visibility::Private),
            ("acme/c", "c", Visibility::Public)
        ]
    );
    let requests = http.requests();
    assert_eq!(
        requests[0].url.expose(),
        "https://hub.docker.com/v2/auth/token"
    );
    assert_eq!(
        requests[1].url.expose(),
        "https://hub.docker.com/v2/namespaces/acme/repositories?page_size=2&page=1"
    );
    assert_eq!(
        header(&requests[1], "Authorization"),
        Some("Bearer «secret:TOKEN»".to_owned())
    );
    let refused = ScriptedHttp::new(vec![answer(401, &json!({}), None)]);
    assert!(matches!(
        listed(&source, &refused, None),
        Err(Unlisted::Failed(Unpolled::Unreadable(_)))
    ));
}

#[test]
fn a_namespace_listing_more_than_its_caps_is_refused_for_the_round() {
    let listing = |most_listed: usize, most_members: usize| {
        namespaced(
            Registry::DockerHub,
            &RegistryOptions {
                most_listed: NonZeroUsize::new(most_listed),
                most_members: NonZeroUsize::new(most_members),
                exclude: vec!["c".to_owned()],
                ..hub(None)
            },
            None,
        )
    };
    let page = || {
        ScriptedHttp::new(vec![answer(
            200,
            &repositories(&[("a", false), ("b", false), ("c", false)], false),
            None,
        )])
    };
    assert_eq!(
        listed(&listing(2, 5), &page(), None).unwrap_err(),
        Unlisted::Over(
            "acme lists more than 2 packages, most_listed; raise it, or name a narrower namespace"
                .to_owned()
        )
    );
    assert_eq!(
        members(&listed(&listing(3, 2), &page(), None).unwrap()).len(),
        2
    );
    assert_eq!(
        listed(&listing(3, 1), &page(), None).unwrap_err(),
        Unlisted::Over(
            "2 packages of acme are taken, more than most_members, 1; raise it, or narrow include and exclude"
                .to_owned()
        )
    );
}

#[test]
fn each_package_of_a_namespace_is_polled_and_announced_as_its_own() {
    let options = RegistryOptions {
        include: vec!["a*".to_owned()],
        visibility: Some(Declared::Public),
        ..hub(None)
    };
    let source = namespaced(Registry::DockerHub, &options, None);
    let official = source.member_of("library/caddy".to_owned(), None);
    assert_eq!(source.project(&official), Ok("caddy".to_owned()));
    let own = source.member_of("acme/api".to_owned(), Some(Visibility::Private));
    assert_eq!(source.project(&own), Ok("acme/api".to_owned()));
    assert!(source.takes(&own));
    assert_eq!(source.owner_key(), "namespace");
    let origin = source.origin(&own);
    assert_eq!(
        (
            origin.forge.as_str(),
            origin.host.as_str(),
            origin.owner.as_str()
        ),
        ("docker-hub", "hub.docker.com", "acme")
    );
    assert_eq!(
        (origin.repository.as_str(), origin.visibility),
        ("acme/api", Visibility::Private)
    );
    assert_eq!(source.member_nouns(), ("package", "packages"));
    assert!(!source.pushed());
    assert!(!source.takes(&source.member_of("acme/web".to_owned(), None)));
    let poll = source.member(&own);
    assert_eq!(poll.api(), "https://hub.docker.com/v2");
    assert_eq!(
        poll.remembered(),
        options.reading(Registry::DockerHub).remembered
    );
    assert_eq!(source.api(), "https://hub.docker.com/v2");
    assert_eq!(source.bases(), &[] as &[String]);
    assert_eq!(source.keep_members(), options.keep_members());
    assert_eq!(source.remembered(), poll.remembered());
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!({"results": [{"name": "1.0", "tag_last_pushed": "2026-01-01T00:00:00Z"}]}),
        None,
    )]);
    let polled = pollster::block_on(poll.polled(&http, None, ROUND))
        .unwrap()
        .unwrap();
    assert_eq!(
        polled.events[0].identity().as_str(),
        "docker.io/acme/api/1.0"
    );
    assert_eq!(
        http.asked()[0].0,
        "https://hub.docker.com/v2/namespaces/acme/repositories/api/tags?page_size=30&ordering=last_updated"
    );
}

#[test]
fn a_namespace_s_fingerprint_follows_what_its_listing_asks_for() {
    let print = |options: &RegistryOptions| namespaced(Registry::Npm, options, None).fingerprint();
    let base = print(&scope(false));
    assert_eq!(base, print(&scope(false)));
    for changed in [
        scope(true),
        RegistryOptions {
            include: vec!["a".to_owned()],
            ..scope(false)
        },
        RegistryOptions {
            scope: Some("@other".to_owned()),
            ..scope(false)
        },
        RegistryOptions {
            api: Some("https://npm.example".to_owned()),
            ..scope(false)
        },
        RegistryOptions {
            token: Some(token()),
            ..scope(false)
        },
    ] {
        assert_ne!(print(&changed), base);
    }
    let printed =
        |options: &RegistryOptions| namespaced(Registry::Packagist, options, None).fingerprint();
    assert_ne!(
        printed(&vendor(None, None)),
        printed(&vendor(None, Some("https://packagist.example")))
    );
    assert_ne!(
        printed(&vendor(None, None)),
        printed(&vendor(Some(VendorListing::Composer), None))
    );
    let crates = |page: usize| {
        namespaced(
            Registry::CratesIo,
            &RegistryOptions {
                listed_page: NonZeroUsize::new(page),
                ..owner("user:o")
            },
            None,
        )
        .fingerprint()
    };
    assert_ne!(crates(2), crates(3));
    assert_ne!(
        namespaced(Registry::Npm, &scope(false), None).fingerprint(),
        namespaced(
            Registry::DockerHub,
            &RegistryOptions {
                namespace: Some("@acme".to_owned()),
                ..RegistryOptions::default()
            },
            None
        )
        .fingerprint()
    );
}

#[test]
fn a_listing_naming_a_package_outside_its_namespace_is_refused_naming_it() {
    let vendor = namespaced(Registry::Packagist, &vendor(None, None), None);
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!({"packageNames": ["ACME/a", "evil/b"]}),
        None,
    )]);
    assert_eq!(
        listed(&vendor, &http, None).unwrap_err(),
        failed(SourceError::Unreadable {
            what: packagist::VENDOR,
            reason: "it names evil/b, outside the vendor acme; check that listing_api lists Packagist's vendors".to_owned()
        })
    );
    let hub = namespaced(Registry::DockerHub, &hub(None), None);
    let foreign = json!({"results": [
        {"name": "a", "namespace": "ACME", "is_private": false},
        {"name": "b", "namespace": "evil", "is_private": false},
    ], "next": null});
    let http = ScriptedHttp::new(vec![answer(200, &foreign, None)]);
    assert_eq!(
        listed(&hub, &http, None).unwrap_err(),
        failed(SourceError::Unreadable {
            what: docker_hub::REPOSITORIES,
            reason: "it names evil/b, outside the namespace acme; check that api lists Docker Hub's namespaces".to_owned()
        })
    );
    let npm = namespaced(
        Registry::Npm,
        &RegistryOptions {
            scope: Some("@ACME".to_owned()),
            ..RegistryOptions::default()
        },
        None,
    );
    let http = ScriptedHttp::new(vec![answer(200, &org(), None)]);
    assert_eq!(
        members(&listed(&npm, &http, None).unwrap()),
        [
            ("@acme/a", "a", Visibility::Public),
            ("@acme/b", "b", Visibility::Public)
        ]
    );
}

#[test]
fn a_listing_naming_what_is_no_package_s_name_is_refused_naming_it() {
    let refused = |registry: Registry,
                   options: &RegistryOptions,
                   answers: Vec<Response>,
                   name: &str| {
        let source = namespaced(registry, options, None);
        assert_eq!(
            listed(&source, &ScriptedHttp::new(answers), None).unwrap_err(),
            failed(SourceError::Unreadable {
                what: registry.listing(),
                reason: format!(
                    "it names \"{name}\", which is not a package's name: {}, of letters, digits, '.', '_' and '-'",
                    registry.shape()
                ),
            }),
            "{name}"
        );
    };
    for name in ["@acme/a/b", "@acme/..", "@acme/%2e%2e", "@acme/"] {
        refused(
            Registry::Npm,
            &scope(false),
            vec![answer(200, &json!({ name: "write" }), None)],
            name,
        );
    }
    for name in ["..", "a%2fb", "a.b/c"] {
        refused(
            Registry::Npm,
            &scope(true),
            vec![answer(200, &json!({ name: "write" }), None)],
            name,
        );
    }
    for name in ["acme/..", "acme/a/b", "acme/", "acme/%2e"] {
        refused(
            Registry::Packagist,
            &vendor(None, None),
            vec![answer(200, &json!({"packageNames": [name]}), None)],
            name,
        );
    }
    for name in ["..", "a/b", "%41", ""] {
        refused(
            Registry::CratesIo,
            &owner("user:o"),
            vec![
                answer(200, &json!({"user": {"id": 1}}), None),
                answer(200, &crates(&[name], false), None),
            ],
            name,
        );
    }
    for (repository, name) in [("..", "acme/.."), ("a/b", "acme/a/b"), ("%2e", "acme/%2e")] {
        refused(
            Registry::DockerHub,
            &hub(None),
            vec![answer(
                200,
                &repositories(&[(repository, false)], false),
                None,
            )],
            name,
        );
    }
}

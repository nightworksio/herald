use std::collections::{BTreeMap, BTreeSet};

use herald_core::{Attribute, Cursor, Identity, Kind, Polled, PrereleaseRule, Signal, Validators};
use herald_kinds::{Poll as _, Unpolled};
use herald_ports::Response;
use serde_json::{Value, json};

use crate::polled::Releases;
use crate::{Forge, Origin, Signals, SourceOptions};
use herald_testing::ScriptedHttp;

use super::Packaging;

/// The round each poll here is made in.
const ROUND: jiff::Timestamp = jiff::Timestamp::UNIX_EPOCH;

fn answer(body: &Value, headers: &[(&str, &str)]) -> Response {
    Response {
        status: 200,
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        body: body.to_string().into_bytes(),
    }
}

fn ok(body: &Value) -> Response {
    answer(body, &[])
}

fn unchanged() -> Response {
    Response {
        status: 304,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn options(forge: Forge, chosen: SourceOptions) -> SourceOptions {
    let api = match forge {
        Forge::GitHub => None,
        Forge::GitLab | Forge::Forgejo => Some("https://forge.example/api".to_owned()),
    };
    SourceOptions {
        repository: Some("o/r".to_owned()),
        api,
        signals: Some(vec![Signal::Package]),
        ..chosen
    }
}

fn releases(forge: Forge, chosen: SourceOptions) -> Releases {
    let options = options(forge, chosen);
    Releases {
        origin: Origin::new(forge, &options, "o/r"),
        token: None,
        project: None,
        signals: Signals {
            taken: options
                .signals
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            prerelease: PrereleaseRule::Either,
        },
        backfill: options.backfill,
        held: None,
        watched: 0,
        packaging: Packaging::of(forge, &options),
        list: super::PackageList::Own,
        visibility: None,
        runs: crate::options::RunChoice::default(),
        alerts: crate::SourceOptions::default().alert_choice(crate::Forge::GitHub),
        pulls: crate::SourceOptions::default().pull_choice(),
        issues: crate::SourceOptions::default().issue_choice(),
    }
}

fn polled(
    releases: &Releases,
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
) -> Result<Option<Polled>, Unpolled> {
    pollster::block_on(releases.polled(http, cursor, ROUND))
}

fn identities(polled: &Polled) -> Vec<&str> {
    polled
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect()
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.asked().into_iter().map(|(url, _)| url).collect()
}

fn text(value: &str) -> Attribute {
    Attribute::Text(value.to_owned())
}

fn polled_packages() -> Cursor {
    Cursor {
        signals: BTreeMap::from([(Signal::Package, Validators::default())]),
        ..Cursor::default()
    }
}

fn github_repository(kind: &str) -> Response {
    ok(&json!({
        "id": 7,
        "name": "r",
        "html_url": "https://github.com/O/r",
        "visibility": "public",
        "owner": {"login": "O", "type": kind}
    }))
}

fn github_package(name: &str, repository: Option<u64>) -> Value {
    json!({
        "name": name,
        "html_url": format!("https://github.com/orgs/O/packages/container/package/{name}"),
        "repository": repository.map(|id| json!({"id": id}))
    })
}

fn container_version(tags: &[&str], created: &str) -> Value {
    json!({
        "name": "sha256:abc",
        "html_url": "https://github.com/orgs/O/packages/container/herald/1",
        "created_at": created,
        "metadata": {"package_type": "container", "container": {"tags": tags}}
    })
}

fn github_containers() -> SourceOptions {
    SourceOptions {
        package_types: Some(vec!["npm".to_owned(), "container".to_owned()]),
        ..SourceOptions::default()
    }
}

#[test]
fn a_github_repository_announces_each_tag_of_its_linked_images_and_each_npm_version() {
    let releases = releases(Forge::GitHub, github_containers());
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        ok(&json!([github_package("cli", Some(7))])),
        ok(&json!([
            github_package("Herald", Some(7)),
            github_package("other", Some(8)),
            github_package("loose", None)
        ])),
        answer(
            &json!([{
                "name": "1.2.0",
                "html_url": "https://github.com/O/r/pkgs/npm/cli",
                "created_at": "2026-01-03T00:00:00Z"
            }]),
            &[("ETag", "\"npm\"")],
        ),
        answer(
            &json!([
                container_version(&["v1.1.0-rc.1"], "2026-01-02T00:00:00Z"),
                container_version(&["v1.0.0", "latest"], "2026-01-01T00:00:00Z"),
                json!({"name": "sha256:def", "created_at": "2025-12-31T00:00:00Z", "metadata": {"container": {"tags": []}}}),
                json!({"name": "sha256:fed", "created_at": "2025-12-30T00:00:00Z"})
            ]),
            &[("ETag", "\"herald\"")],
        ),
    ]);
    let found = polled(&releases, &http, None).unwrap().unwrap();
    assert_eq!(
        identities(&found),
        [
            "github.com/npm/O/cli/1.2.0",
            "ghcr.io/o/herald/v1.1.0-rc.1",
            "ghcr.io/o/herald/v1.0.0"
        ]
    );
    let rc = &found.events[1];
    assert_eq!(rc.kind(), &Kind::PACKAGE_VERSION);
    assert_eq!(rc.project(), "r");
    assert_eq!(rc.title(), "Herald v1.1.0-rc.1");
    assert_eq!(rc.signal(), Some(Signal::Package));
    assert!(rc.prerelease());
    assert!(!found.events[2].prerelease());
    assert_eq!(rc.attribute(herald_core::PACKAGE), Some(&text("Herald")));
    assert_eq!(
        rc.attribute(herald_core::PACKAGE_TYPE),
        Some(&text("container"))
    );
    assert_eq!(
        rc.link("release"),
        Some("https://github.com/orgs/O/packages/container/herald/1")
    );
    assert_eq!(
        urls(&http),
        [
            "https://api.github.com/repos/o/r",
            "https://api.github.com/orgs/O/packages?package_type=npm&per_page=100&page=1",
            "https://api.github.com/orgs/O/packages?package_type=container&per_page=100&page=1",
            "https://api.github.com/orgs/O/packages/npm/cli/versions?per_page=30",
            "https://api.github.com/orgs/O/packages/container/Herald/versions?per_page=30",
        ]
    );
    assert_eq!(
        found.packages.keys().collect::<Vec<_>>(),
        ["container/Herald", "npm/cli"]
    );
    assert_eq!(found.packages["npm/cli"].etag.as_deref(), Some("\"npm\""));
    assert_eq!(
        found.signals.get(&Signal::Package),
        Some(&Validators::default())
    );
    assert_eq!(found.unread, Vec::<Identity>::new());
}

#[test]
fn a_github_user_s_packages_are_listed_page_by_page_within_the_most_listed() {
    let chosen = || SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        packages: vec!["her*".to_owned()],
        moving: Some(vec!["edge".to_owned()]),
        registry: Some("registry.example:5000".to_owned()),
        most_packages: std::num::NonZeroUsize::new(3),
        ..SourceOptions::default()
    };
    let next = [("Link", "<https://api.github.com/next>; rel=\"next\"")];
    let http = ScriptedHttp::new(vec![
        github_repository("User"),
        answer(
            &json!([
                github_package("herald", Some(7)),
                github_package("cli", Some(7))
            ]),
            &next,
        ),
        ok(&json!([github_package("herald-cli", Some(7))])),
        ok(&json!([container_version(
            &["edge", "v2"],
            "2026-01-01T00:00:00Z"
        )])),
        ok(&json!([])),
    ]);
    let found = polled(&releases(Forge::GitHub, chosen()), &http, None)
        .unwrap()
        .unwrap();
    assert_eq!(identities(&found), ["registry.example:5000/o/herald/v2"]);
    assert_eq!(
        urls(&http)[1..3],
        [
            "https://api.github.com/users/O/packages?package_type=container&per_page=100&page=1",
            "https://api.github.com/users/O/packages?package_type=container&per_page=100&page=2",
        ]
    );
    let over = ScriptedHttp::new(vec![
        github_repository("User"),
        answer(
            &json!([github_package("a", None), github_package("b", None)]),
            &next,
        ),
        answer(
            &json!([github_package("c", None), github_package("d", None)]),
            &next,
        ),
    ]);
    assert_eq!(
        polled(&releases(Forge::GitHub, chosen()), &over, None),
        Err(Unpolled::Unreadable(
            "GitHub's packages list more than 3 entries, most_packages; raise it, or narrow package_types".to_owned()
        ))
    );
}

#[test]
fn a_package_whose_versions_are_unchanged_keeps_its_validators_and_announces_nothing() {
    let chosen = || SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    let held = Validators {
        etag: Some("\"herald\"".to_owned()),
        last_modified: None,
    };
    let cursor = Cursor {
        packages: BTreeMap::from([("container/herald".to_owned(), held.clone())]),
        ..polled_packages()
    };
    let listing = || ok(&json!([github_package("herald", Some(7))]));
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        listing(),
        unchanged(),
    ]);
    assert_eq!(
        polled(&releases(Forge::GitHub, chosen()), &http, Some(&cursor)),
        Ok(None)
    );
    assert_eq!(
        http.asked()[2].1.as_deref(),
        Some("\"herald\""),
        "the versions are asked conditionally"
    );
    let gone = Cursor {
        packages: BTreeMap::from([
            ("container/herald".to_owned(), held.clone()),
            ("container/old".to_owned(), held.clone()),
        ]),
        ..polled_packages()
    };
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        listing(),
        unchanged(),
    ]);
    let found = polled(&releases(Forge::GitHub, chosen()), &http, Some(&gone))
        .unwrap()
        .unwrap();
    assert_eq!(identities(&found), Vec::<&str>::new());
    assert_eq!(
        found.packages,
        BTreeMap::from([("container/herald".to_owned(), held)])
    );
}

#[test]
fn packages_first_polled_on_a_source_with_a_cursor_are_remembered_unannounced() {
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        ok(&json!([github_package("herald", Some(7))])),
        ok(&json!([container_version(&["v1"], "2026-01-01T00:00:00Z")])),
    ]);
    let chosen = SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    let found = polled(
        &releases(Forge::GitHub, chosen),
        &http,
        Some(&Cursor::default()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(identities(&found), Vec::<&str>::new());
    assert_eq!(found.unread, [Identity::new("ghcr.io/o/herald/v1")]);
    assert_eq!(
        found.signals.get(&Signal::Package),
        Some(&Validators::default())
    );
}

#[test]
fn a_round_keeps_the_newest_page_of_package_versions() {
    let chosen = || SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        page: std::num::NonZeroUsize::new(2),
        ..SourceOptions::default()
    };
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        ok(&json!([github_package("herald", Some(7))])),
        ok(&json!([
            container_version(&["v1"], "2026-01-01T00:00:00Z"),
            container_version(&["v3"], "2026-01-03T00:00:00Z"),
            container_version(&["v2"], "2026-01-02T00:00:00Z")
        ])),
    ]);
    let found = polled(&releases(Forge::GitHub, chosen()), &http, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        identities(&found),
        ["ghcr.io/o/herald/v3", "ghcr.io/o/herald/v2"]
    );
    assert_eq!(found.unread, [Identity::new("ghcr.io/o/herald/v1")]);
    let cursor = Cursor::default().advanced(&found, 500);
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        ok(&json!([github_package("herald", Some(7))])),
        ok(&json!([
            container_version(&["v1"], "2026-01-01T00:00:00Z"),
            container_version(&["v3"], "2026-01-03T00:00:00Z"),
            container_version(&["v2"], "2026-01-02T00:00:00Z")
        ])),
    ]);
    assert_eq!(
        polled(&releases(Forge::GitHub, chosen()), &http, Some(&cursor)),
        Ok(None),
        "what a round remembers changes nothing the next"
    );
}

#[test]
fn a_repository_its_forge_names_no_owner_or_page_of_polls_no_packages() {
    let chosen = || SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    for (forge, described, reason) in [
        (
            Forge::GitHub,
            json!({"id": 7, "name": "r", "html_url": "https://github.com/O/r"}),
            "GitHub's repository names no owner",
        ),
        (
            Forge::Forgejo,
            json!({"id": 7, "name": "r", "html_url": "https://forge.example/O/r"}),
            "Forgejo's repository names no owner",
        ),
        (
            Forge::GitLab,
            json!({"id": 7, "name": "r", "web_url": "https://forge.example/o/r"}),
            "GitLab's project names no full path",
        ),
        (
            Forge::GitLab,
            json!({"id": 7, "name": "r", "path_with_namespace": "o/r"}),
            "GitLab's project names no web page",
        ),
    ] {
        let http = ScriptedHttp::new(vec![ok(&described)]);
        assert_eq!(
            polled(&releases(forge, chosen()), &http, None),
            Err(Unpolled::Unreadable(reason.to_owned())),
            "{forge:?}"
        );
    }
}

fn gitlab_project() -> Response {
    ok(&json!({
        "id": 7,
        "name": "r",
        "web_url": "https://gitlab.example/g/s/r",
        "path_with_namespace": "g/s/r"
    }))
}

#[test]
fn a_gitlab_project_announces_its_packages_and_each_unseen_tag_of_its_images() {
    let cursor = Cursor {
        seen: vec![Identity::new("registry.gitlab.example/g/s/r/v1")],
        ..polled_packages()
    };
    let http = ScriptedHttp::new(vec![
        gitlab_project(),
        answer(
            &json!([
                {"name": "@g/cli", "version": "2.0.0", "package_type": "npm", "created_at": "2026-01-04T00:00:00Z", "_links": {"web_path": "/g/s/r/-/packages/9"}},
                {"name": "lib", "version": "1.0.0", "package_type": "pypi", "created_at": "2026-01-03T00:00:00Z"},
                {"name": "skipped", "version": "1.0.0", "package_type": "npm", "created_at": "2026-01-02T00:00:00Z"}
            ]),
            &[("ETag", "\"listing\"")],
        ),
        ok(
            &json!([{"id": 3, "name": ""}, {"id": 4, "name": "tools"}, {"id": 5, "name": "unwanted"}]),
        ),
        ok(&json!([
            {"name": "v1", "location": "registry.gitlab.example/g/s/r:v1"},
            {"name": "v2", "location": "registry.gitlab.example/g/s/r:v2"},
            {"name": "latest", "location": "registry.gitlab.example/g/s/r:latest"}
        ])),
        ok(
            &json!({"name": "v2", "location": "registry.gitlab.example/g/s/r:v2", "created_at": "2026-01-05T00:00:00Z"}),
        ),
        ok(&json!([{"name": "v9", "location": "registry.gitlab.example/g/s/r/tools:v9"}])),
        ok(
            &json!({"name": "v9", "location": "registry.gitlab.example/g/s/r/tools:v9", "created_at": "2026-01-01T00:00:00Z"}),
        ),
    ]);
    let chosen = SourceOptions {
        packages: vec![
            "@g/cli".to_owned(),
            "lib".to_owned(),
            "g/s/r".to_owned(),
            "g/s/r/tools".to_owned(),
        ],
        package_types: Some(vec!["npm".to_owned(), "container".to_owned()]),
        ..SourceOptions::default()
    };
    let found = polled(&releases(Forge::GitLab, chosen), &http, Some(&cursor))
        .unwrap()
        .unwrap();
    assert_eq!(
        identities(&found),
        [
            "registry.gitlab.example/g/s/r/v2",
            "gitlab.example/npm/g/s/@g/cli/2.0.0",
            "registry.gitlab.example/g/s/r/tools/v9"
        ]
    );
    assert_eq!(
        found.events[1].link("release"),
        Some("https://gitlab.example/g/s/r/-/packages/9")
    );
    assert_eq!(
        found.events[0].link("release"),
        Some("https://gitlab.example/g/s/r/container_registry/3")
    );
    assert_eq!(found.events[0].title(), "g/s/r v2");
    assert_eq!(
        urls(&http),
        [
            "https://forge.example/api/projects/o%2Fr",
            "https://forge.example/api/projects/o%2Fr/packages?order_by=created_at&sort=desc&per_page=30",
            "https://forge.example/api/projects/o%2Fr/registry/repositories?per_page=100&page=1",
            "https://forge.example/api/projects/o%2Fr/registry/repositories/3/tags?per_page=100&page=1",
            "https://forge.example/api/projects/o%2Fr/registry/repositories/3/tags/v2",
            "https://forge.example/api/projects/o%2Fr/registry/repositories/4/tags?per_page=100&page=1",
            "https://forge.example/api/projects/o%2Fr/registry/repositories/4/tags/v9",
        ]
    );
    assert_eq!(
        found.packages["packages"].etag.as_deref(),
        Some("\"listing\"")
    );
}

#[test]
fn a_gitlab_listing_unchanged_keeps_its_validators_and_images_unread_wait_for_a_cursor() {
    let held = Validators {
        etag: Some("\"listing\"".to_owned()),
        last_modified: None,
    };
    let cursor = Cursor {
        packages: BTreeMap::from([("packages".to_owned(), held.clone())]),
        ..polled_packages()
    };
    let only_npm = || SourceOptions {
        package_types: Some(vec!["npm".to_owned()]),
        ..SourceOptions::default()
    };
    let http = ScriptedHttp::new(vec![gitlab_project(), unchanged()]);
    assert_eq!(
        polled(&releases(Forge::GitLab, only_npm()), &http, Some(&cursor)),
        Ok(None)
    );
    assert_eq!(
        http.asked()[1],
        (
            "https://forge.example/api/projects/o%2Fr/packages?order_by=created_at&sort=desc&per_page=30&package_type=npm".to_owned(),
            Some("\"listing\"".to_owned())
        )
    );
    let images = SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        registry: Some("mirror.example".to_owned()),
        ..SourceOptions::default()
    };
    let http = ScriptedHttp::new(vec![
        gitlab_project(),
        ok(&json!([{"id": 3, "name": ""}])),
        ok(&json!([{"name": "v1", "location": "registry.gitlab.example/g/s/r:v1"}])),
    ]);
    let found = polled(&releases(Forge::GitLab, images), &http, None)
        .unwrap()
        .unwrap();
    assert_eq!(identities(&found), Vec::<&str>::new());
    assert_eq!(found.unread, [Identity::new("mirror.example/g/s/r/v1")]);
}

#[test]
fn a_gitlab_tag_with_no_time_polls_nothing() {
    let http = ScriptedHttp::new(vec![
        gitlab_project(),
        ok(&json!([{"id": 3, "name": ""}])),
        ok(&json!([{"name": "v1", "location": "registry.gitlab.example/g/s/r:v1"}])),
        ok(&json!({"name": "v1", "location": "registry.gitlab.example/g/s/r:v1"})),
    ]);
    let images = SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    assert_eq!(
        polled(
            &releases(Forge::GitLab, images),
            &http,
            Some(&polled_packages())
        ),
        Err(Unpolled::Unreadable(
            "GitLab's image tag v1 has no creation time".to_owned()
        ))
    );
}

fn forgejo_package(kind: &str, (name, version): (&str, &str), repository: Option<u64>) -> Value {
    json!({
        "repository": repository.map(|id| json!({"id": id})),
        "type": kind,
        "name": name,
        "version": version,
        "created_at": "2026-01-01T00:00:00Z",
        "html_url": format!("https://forge.example/O/-/packages/{kind}/{name}/{version}")
    })
}

#[test]
fn a_forgejo_repository_announces_its_owner_s_packages_linked_to_it_until_a_page_is_found() {
    let chosen = SourceOptions {
        page: std::num::NonZeroUsize::new(2),
        ..SourceOptions::default()
    };
    let next = [("Link", "<https://forge.example/next>; rel=\"next\"")];
    let http = ScriptedHttp::new(vec![
        ok(&json!({
            "id": 7,
            "name": "r",
            "html_url": "https://forge.example/O/r",
            "owner": {"login": "O"}
        })),
        answer(
            &json!([
                forgejo_package("container", ("herald", "sha256:abc"), Some(7)),
                forgejo_package("container", ("herald", "latest"), Some(7)),
                forgejo_package("container", ("Herald", "v1"), Some(7)),
                forgejo_package("npm", ("other", "1.0.0"), Some(8)),
                forgejo_package("npm", ("loose", "1.0.0"), None)
            ]),
            &next,
        ),
        answer(
            &json!([forgejo_package("cargo", ("herald", "0.1.0"), Some(7))]),
            &next,
        ),
    ]);
    let found = polled(&releases(Forge::Forgejo, chosen), &http, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        identities(&found),
        [
            "forge.example/o/herald/v1",
            "forge.example/cargo/O/herald/0.1.0"
        ]
    );
    assert_eq!(
        urls(&http)[1..],
        [
            "https://forge.example/api/packages/O?limit=50&page=1",
            "https://forge.example/api/packages/O?limit=50&page=2",
        ]
    );
}

#[test]
fn a_forgejo_listing_of_one_type_asks_for_it_alone_within_the_most_listed() {
    let chosen = || SourceOptions {
        package_types: Some(vec!["npm".to_owned()]),
        most_packages: std::num::NonZeroUsize::new(1),
        ..SourceOptions::default()
    };
    let described = || {
        ok(&json!({
            "id": 7,
            "name": "r",
            "html_url": "https://forge.example/O/r",
            "owner": {"login": "O"}
        }))
    };
    let http = ScriptedHttp::new(vec![
        described(),
        ok(&json!([forgejo_package("npm", ("cli", "1.0.0"), Some(7))])),
    ]);
    let found = polled(&releases(Forge::Forgejo, chosen()), &http, None)
        .unwrap()
        .unwrap();
    assert_eq!(identities(&found), ["forge.example/npm/O/cli/1.0.0"]);
    assert_eq!(
        urls(&http)[1],
        "https://forge.example/api/packages/O?limit=50&page=1&type=npm"
    );
    let over = ScriptedHttp::new(vec![
        described(),
        ok(&json!([
            forgejo_package("npm", ("cli", "1.0.0"), Some(7)),
            forgejo_package("npm", ("cli", "0.9.0"), Some(7))
        ])),
    ]);
    assert_eq!(
        polled(&releases(Forge::Forgejo, chosen()), &over, None),
        Err(Unpolled::Unreadable(
            "Forgejo's packages list more than 1 entries, most_packages; raise it, or narrow package_types".to_owned()
        ))
    );
}

#[test]
fn a_gitlab_project_s_images_are_listed_page_by_page_within_the_most_listed() {
    let chosen = |most: usize| SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        most_packages: std::num::NonZeroUsize::new(most),
        ..SourceOptions::default()
    };
    let next = [("Link", "<https://forge.example/next>; rel=\"next\"")];
    let http = ScriptedHttp::new(vec![
        gitlab_project(),
        answer(&json!([{"id": 3, "name": ""}]), &next),
        ok(&json!([{"id": 4, "name": "tools"}])),
        ok(&json!([])),
        ok(&json!([])),
    ]);
    assert_eq!(
        polled(
            &releases(Forge::GitLab, chosen(2)),
            &http,
            Some(&polled_packages())
        ),
        Ok(None)
    );
    assert_eq!(
        urls(&http)[1..3],
        [
            "https://forge.example/api/projects/o%2Fr/registry/repositories?per_page=100&page=1",
            "https://forge.example/api/projects/o%2Fr/registry/repositories?per_page=100&page=2",
        ]
    );
    let over = ScriptedHttp::new(vec![
        gitlab_project(),
        answer(
            &json!([{"id": 3, "name": ""}, {"id": 4, "name": "tools"}]),
            &next,
        ),
    ]);
    assert_eq!(
        polled(&releases(Forge::GitLab, chosen(1)), &over, None),
        Err(Unpolled::Unreadable(
            "GitLab's container images list more than 1 entries, most_packages; raise it, or narrow package_types".to_owned()
        ))
    );
}

#[test]
fn a_poll_tells_a_change_by_a_version_unseen_in_or_beyond_its_page() {
    let chosen = || SourceOptions {
        page: std::num::NonZeroUsize::new(1),
        remembered: std::num::NonZeroUsize::new(1),
        ..SourceOptions::default()
    };
    let described = || {
        ok(&json!({
            "id": 7,
            "name": "r",
            "html_url": "https://forge.example/O/r",
            "owner": {"login": "O"}
        }))
    };
    let mut newer = forgejo_package("npm", ("cli", "2.0.0"), Some(7));
    newer["created_at"] = json!("2026-01-02T00:00:00Z");
    let older = forgejo_package("npm", ("cli", "1.0.0"), Some(7));
    let script = || {
        ScriptedHttp::new(vec![
            described(),
            ok(&json!([newer.clone(), older.clone()])),
        ])
    };
    let seen = |identities: &[&str]| Cursor {
        seen: identities.iter().map(|seen| Identity::new(*seen)).collect(),
        ..polled_packages()
    };
    let (two, one) = (
        "forge.example/npm/O/cli/2.0.0",
        "forge.example/npm/O/cli/1.0.0",
    );
    assert_eq!(
        polled(
            &releases(Forge::Forgejo, chosen()),
            &script(),
            Some(&seen(&[two, one]))
        ),
        Ok(None)
    );
    let found = polled(
        &releases(Forge::Forgejo, chosen()),
        &script(),
        Some(&seen(&[one])),
    )
    .unwrap()
    .unwrap();
    assert_eq!(identities(&found), [two]);
    let found = polled(
        &releases(Forge::Forgejo, chosen()),
        &script(),
        Some(&seen(&[two])),
    )
    .unwrap()
    .unwrap();
    assert_eq!(identities(&found), [two]);
    assert_eq!(found.unread, [Identity::new(one)]);
}

fn seen_of(polled: &Polled) -> Vec<Option<herald_core::Visibility>> {
    polled
        .events
        .iter()
        .map(herald_core::Event::visibility)
        .collect()
}

#[test]
fn each_package_is_seen_as_the_stricter_of_its_own_and_its_repository_s_visibility() {
    use herald_core::Visibility::{Internal, Private, Public};
    let image = |name: &str, visibility: Option<&str>| {
        let mut package = github_package(name, Some(7));
        package["visibility"] = json!(visibility);
        package
    };
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        ok(&json!([
            image("open", Some("public")),
            image("closed", Some("private")),
            image("staff", Some("internal")),
            image("odd", Some("secret")),
            image("unnamed", None)
        ])),
        ok(&json!([container_version(&["v5"], "2026-01-05T00:00:00Z")])),
        ok(&json!([container_version(&["v4"], "2026-01-04T00:00:00Z")])),
        ok(&json!([container_version(&["v3"], "2026-01-03T00:00:00Z")])),
        ok(&json!([container_version(&["v2"], "2026-01-02T00:00:00Z")])),
        ok(&json!([container_version(&["v1"], "2026-01-01T00:00:00Z")])),
    ]);
    let containers = SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    let found = polled(&releases(Forge::GitHub, containers), &http, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        seen_of(&found),
        [
            Some(Public),
            Some(Private),
            Some(Internal),
            Some(Private),
            Some(Private)
        ]
    );
    for (project, expected) in [
        (Some("public"), Public),
        (Some("internal"), Internal),
        (Some("private"), Private),
        (None, Private),
    ] {
        let mut described = json!({
            "id": 7,
            "name": "r",
            "web_url": "https://gitlab.example/g/r",
            "path_with_namespace": "g/r"
        });
        described["visibility"] = json!(project);
        let http = ScriptedHttp::new(vec![
            ok(&described),
            ok(
                &json!([{"name": "cli", "version": "1.0.0", "package_type": "npm", "created_at": "2026-01-01T00:00:00Z"}]),
            ),
        ]);
        let only_npm = SourceOptions {
            package_types: Some(vec!["npm".to_owned()]),
            ..SourceOptions::default()
        };
        let found = polled(&releases(Forge::GitLab, only_npm), &http, None)
            .unwrap()
            .unwrap();
        assert_eq!(seen_of(&found), [Some(expected)], "{project:?}");
    }
    for (owner, expected) in [
        (json!({"visibility": "public"}), Public),
        (json!({"visibility": "limited"}), Internal),
        (json!({"visibility": "private"}), Private),
        (json!({}), Private),
        (json!(null), Private),
    ] {
        let mut package = forgejo_package("npm", ("cli", "1.0.0"), Some(7));
        package["owner"] = owner.clone();
        let http = ScriptedHttp::new(vec![
            ok(
                &json!({"id": 7, "name": "r", "html_url": "https://forge.example/O/r", "private": false, "owner": {"login": "O"}}),
            ),
            ok(&json!([package])),
        ]);
        let found = polled(
            &releases(Forge::Forgejo, SourceOptions::default()),
            &http,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(seen_of(&found), [Some(expected)], "{owner}");
    }
}

#[test]
fn a_public_package_of_a_private_repository_is_private_unless_the_package_decides() {
    use herald_core::Visibility::{Internal, Private, Public};
    for (described, chosen, expected) in [
        (json!({"visibility": "private"}), None, Private),
        (
            json!({"visibility": "private"}),
            Some(crate::PackageVisibility::Stricter),
            Private,
        ),
        (
            json!({"visibility": "private"}),
            Some(crate::PackageVisibility::Package),
            Public,
        ),
        (json!({"private": false, "internal": true}), None, Internal),
        (json!({"private": false}), None, Public),
        (json!({}), None, Private),
    ] {
        let mut repository = json!({
            "id": 7,
            "name": "r",
            "html_url": "https://github.com/O/r",
            "owner": {"login": "O", "type": "Organization"}
        });
        for (key, value) in described.as_object().unwrap() {
            repository[key] = value.clone();
        }
        let mut package = github_package("herald", Some(7));
        package["visibility"] = json!("public");
        let http = ScriptedHttp::new(vec![
            ok(&repository),
            ok(&json!([package])),
            ok(&json!([container_version(&["v1"], "2026-01-01T00:00:00Z")])),
        ]);
        let options = SourceOptions {
            package_types: Some(vec!["container".to_owned()]),
            package_visibility: chosen,
            ..SourceOptions::default()
        };
        let found = polled(&releases(Forge::GitHub, options), &http, None)
            .unwrap()
            .unwrap();
        assert_eq!(seen_of(&found), [Some(expected)], "{described} {chosen:?}");
    }
}

#[test]
fn a_version_whose_page_is_no_web_address_polls_nothing() {
    let mut version = container_version(&["v1"], "2026-01-01T00:00:00Z");
    version["html_url"] = json!("not a page");
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        ok(&json!([github_package("herald", Some(7))])),
        ok(&json!([version])),
    ]);
    let chosen = SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    assert!(matches!(
        polled(&releases(Forge::GitHub, chosen), &http, None),
        Err(Unpolled::Unreadable(reason)) if reason.starts_with("GitHub's package versions")
    ));
}

#[test]
fn a_rate_limited_package_listing_stops_the_poll_as_one() {
    let http = ScriptedHttp::new(vec![
        github_repository("Organization"),
        Response {
            status: 429,
            headers: Vec::new(),
            body: Vec::new(),
        },
    ]);
    let chosen = SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    assert!(matches!(
        polled(&releases(Forge::GitHub, chosen), &http, None),
        Err(Unpolled::RateLimited(reason)) if reason.contains("GitHub's packages")
    ));
}

mod dotted;

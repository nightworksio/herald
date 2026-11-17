//! An owner's packages, listed once a round and polled with each member.

use herald_core::{Signal, Validators};

use super::*;

fn member(id: &str, name: &str) -> Listed {
    Listed {
        id: id.to_owned(),
        name: name.to_owned(),
        path: format!("lemonfiber/{name}"),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

fn packaged(forge: Forge, chosen: SourceOptions) -> Owned {
    owned(
        forge,
        &SourceOptions {
            signals: Some(vec![Signal::Package]),
            ..chosen
        },
    )
}

fn rounded(source: &Owned, http: &ScriptedHttp, owner: Owner) -> Result<Rounded, Unpolled> {
    pollster::block_on(source.rounded(http, owner))
}

fn identities(polled: &Result<Option<Polled>, Unpolled>) -> Vec<String> {
    polled
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(|polled| {
            polled
                .events
                .iter()
                .map(|event| event.identity().as_str().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn forgejo_version(name: &str, version: &str, repository: u64) -> Value {
    json!({
        "repository": {"id": repository},
        "type": "npm",
        "name": name,
        "version": version,
        "created_at": "2026-01-01T00:00:00Z",
        "html_url": format!("https://codeberg.org/lemonfiber/-/packages/npm/{name}/{version}")
    })
}

#[test]
fn an_owner_reads_its_packages_once_a_round_only_where_a_member_announces_them() {
    let nothing = ScriptedHttp::new(Vec::new());
    for source in [
        packaged(Forge::GitLab, SourceOptions::default()),
        owned(Forge::GitHub, &SourceOptions::default()),
    ] {
        assert!(matches!(
            rounded(&source, &nothing, Owner::Organization),
            Ok(Rounded::Nothing)
        ));
    }
    assert_eq!(nothing.asked(), Vec::new());
    let containers = || SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            {"name": "herald", "html_url": "https://github.com/herald", "repository": {"id": 9}},
            {"name": "loose", "html_url": "https://github.com/loose"}
        ]),
        "",
        false,
    )]);
    let read = rounded(&packaged(Forge::GitHub, containers()), &http, Owner::User).unwrap();
    assert_eq!(
        read.named(),
        Some(&std::collections::BTreeSet::from(["9".to_owned()]))
    );
    assert_eq!(
        http.asked()[0].0,
        "https://api.github.com/users/lemonfiber/packages?package_type=container&per_page=100&page=1"
    );
    let failing = ScriptedHttp::new(vec![answer(403, &json!({}), "", false)]);
    assert!(
        rounded(
            &packaged(Forge::GitHub, containers()),
            &failing,
            Owner::Organization
        )
        .is_err()
    );
    assert_eq!(
        failing.asked()[0].0,
        "https://api.github.com/orgs/lemonfiber/packages?package_type=container&per_page=100&page=1"
    );
}

#[test]
fn a_member_polled_on_its_own_takes_its_packages_from_the_owner_s_listing() {
    let source = packaged(Forge::Forgejo, SourceOptions::default());
    let listing = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            forgejo_version("cli", "1.0.0", 9),
            forgejo_version("web", "2.0.0", 10)
        ]),
        "",
        false,
    )]);
    let read = rounded(&source, &listing, Owner::Organization).unwrap();
    assert_eq!(
        listing.asked()[0].0,
        "https://codeberg.org/api/v1/packages/lemonfiber?limit=50&page=1"
    );
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!({"id": 9, "name": "cli", "html_url": "https://codeberg.org/lemonfiber/cli"}),
        "",
        false,
    )]);
    let cli = member("9", "cli");
    let polled = pollster::block_on(source.polled(&http, &[(&cli, None)], &read));
    assert_eq!(
        identities(&polled[0]),
        ["codeberg.org/npm/lemonfiber/cli/1.0.0"]
    );
    assert_eq!(
        http.asked()
            .into_iter()
            .map(|(url, _)| url)
            .collect::<Vec<_>>(),
        ["https://codeberg.org/api/v1/repos/lemonfiber/cli"],
        "the member lists nothing of its own"
    );
    let marked = Cursor {
        signals: BTreeMap::from([(Signal::Package, Validators::default())]),
        ..Cursor::default()
    };
    let again = ScriptedHttp::new(vec![answer(
        200,
        &json!({"id": 9, "name": "cli", "html_url": "https://codeberg.org/lemonfiber/cli"}),
        "",
        false,
    )]);
    let polled = pollster::block_on(source.polled(&again, &[(&cli, Some(&marked))], &read));
    assert_eq!(
        identities(&polled[0]),
        ["codeberg.org/npm/lemonfiber/cli/1.0.0"],
        "a member the listing names is read though polled for packages before"
    );
    let docs = member("11", "docs");
    let none = ScriptedHttp::new(Vec::new());
    assert_eq!(
        pollster::block_on(source.polled(&none, &[(&docs, Some(&marked))], &read)),
        [Ok(None)],
        "a member with no package, polled for them before, asks nothing"
    );
    assert_eq!(none.asked(), Vec::new());
    let first = ScriptedHttp::new(vec![answer(
        200,
        &json!({"id": 11, "name": "docs", "html_url": "https://codeberg.org/lemonfiber/docs"}),
        "",
        false,
    )]);
    let polled =
        pollster::block_on(source.polled(&first, &[(&docs, Some(&Cursor::default()))], &read));
    let Ok(Some(found)) = &polled[0] else {
        panic!("a member polled for packages the first time is marked: {polled:?}");
    };
    assert_eq!(
        found.signals.get(&Signal::Package),
        Some(&Validators::default())
    );
    let unread = ScriptedHttp::new(Vec::new());
    let foreign = Rounded::Read {
        named: std::collections::BTreeSet::new(),
        read: Arc::new(()),
    };
    for rounded in [Rounded::Unread, foreign] {
        assert_eq!(
            pollster::block_on(source.polled(&unread, &[(&cli, None)], &rounded)),
            [Ok(None)]
        );
    }
    assert_eq!(unread.asked(), Vec::new());
}

fn graphql_owner() -> Owned {
    owned(
        Forge::GitHub,
        &SourceOptions {
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            signals: Some(vec![Signal::Release, Signal::Package]),
            package_types: Some(vec!["container".to_owned()]),
            ..SourceOptions::default()
        },
    )
}

fn released() -> Response {
    let releases = json!({ "databaseId": 9, "releases": { "nodes": [{
        "tagName": "v1",
        "url": "https://github.com/lemonfiber/cli/releases/tag/v1",
        "isDraft": false,
        "publishedAt": "2026-11-01T12:00:00Z"
    }] } });
    answer(200, &json!({ "data": { "m0": releases } }), "", false)
}

#[test]
fn a_member_polled_through_graphql_takes_its_packages_from_the_owner_s_listing() {
    let source = graphql_owner();
    let listing = ScriptedHttp::new(vec![answer(
        200,
        &json!([{"name": "cli", "html_url": "https://github.com/cli", "repository": {"id": 9}}]),
        "",
        false,
    )]);
    let read = rounded(&source, &listing, Owner::Organization).unwrap();
    let versions = || {
        answer(
            200,
            &json!([{
                "name": "sha256:abc",
                "created_at": "2026-11-02T12:00:00Z",
                "metadata": {"container": {"tags": ["v1.1"]}}
            }]),
            "\"cli\"",
            false,
        )
    };
    let cli = member("9", "cli");
    let http = ScriptedHttp::new(vec![released(), released(), versions()]);
    let polled = pollster::block_on(source.polled(&http, &[(&cli, None)], &read));
    assert_eq!(
        identities(&polled[0]),
        ["ghcr.io/lemonfiber/cli/v1.1", "github.com/9/v1"]
    );
    assert_eq!(
        http.asked()[2].0,
        "https://api.github.com/orgs/lemonfiber/packages/container/cli/versions?per_page=30"
    );
    let cursor = Cursor {
        seen: vec![herald_core::Identity::new("github.com/9/v1")],
        etag: Some("\"releases\"".to_owned()),
        last_modified: Some("then".to_owned()),
        signals: BTreeMap::from([
            (Signal::Package, Validators::default()),
            (
                Signal::Tag,
                Validators {
                    etag: Some("\"tags\"".to_owned()),
                    last_modified: None,
                },
            ),
        ]),
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![released(), versions()]);
    let polled = pollster::block_on(source.polled(&http, &[(&cli, Some(&cursor))], &read));
    assert_eq!(identities(&polled[0]), ["ghcr.io/lemonfiber/cli/v1.1"]);
    let Ok(Some(found)) = &polled[0] else {
        panic!("the member's new image is found");
    };
    assert_eq!(found.etag.as_deref(), Some("\"releases\""));
    assert_eq!(found.last_modified.as_deref(), Some("then"));
    assert_eq!(
        found.events[0].visibility(),
        Some(Visibility::Private),
        "a package GitHub names no visibility of is private"
    );
    assert_eq!(found.signals, cursor.signals);
    assert_eq!(
        found.packages["container/cli"].etag.as_deref(),
        Some("\"cli\"")
    );
    let quiet = ScriptedHttp::new(vec![released()]);
    assert_eq!(
        pollster::block_on(source.polled(&quiet, &[(&cli, Some(&cursor))], &Rounded::Unread)),
        [Ok(None)]
    );
    let failing = ScriptedHttp::new(vec![released()]);
    assert!(
        pollster::block_on(source.polled(&failing, &[(&cli, Some(&cursor))], &read))[0].is_err()
    );
}

#[test]
fn a_member_whose_signals_take_no_package_keeps_its_cursor_as_it_was() {
    let source = owned(
        Forge::GitHub,
        &SourceOptions {
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            signals: Some(vec![Signal::Release, Signal::Package]),
            package_types: Some(vec!["container".to_owned()]),
            repositories: BTreeMap::from([(
                "cli".to_owned(),
                crate::Override {
                    signals: Some(vec![Signal::Release]),
                    ..crate::Override::default()
                },
            )]),
            ..SourceOptions::default()
        },
    );
    let listing = ScriptedHttp::new(vec![answer(200, &json!([]), "", false)]);
    let read = rounded(&source, &listing, Owner::Organization).unwrap();
    let cli = member("9", "cli");
    let cursor = Cursor {
        seen: vec![herald_core::Identity::new("github.com/9/v1")],
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![released()]);
    assert_eq!(
        pollster::block_on(source.polled(&http, &[(&cli, Some(&cursor))], &read)),
        [Ok(None)]
    );
}

#[test]
fn a_member_whose_owner_s_listing_failed_keeps_its_package_state_as_its_releases_change() {
    let source = owned(
        Forge::GitHub,
        &SourceOptions {
            signals: Some(vec![Signal::Release, Signal::Package]),
            package_types: Some(vec!["container".to_owned()]),
            ..SourceOptions::default()
        },
    );
    let held = Validators {
        etag: Some("\"cli\"".to_owned()),
        last_modified: None,
    };
    let cursor = Cursor {
        signals: BTreeMap::from([(Signal::Package, Validators::default())]),
        packages: BTreeMap::from([("container/cli".to_owned(), held)]),
        ..Cursor::default()
    };
    let releases = json!([{
        "tag_name": "v2",
        "name": null,
        "body": "notes",
        "html_url": "https://github.com/lemonfiber/cli/releases/tag/v2",
        "draft": false,
        "prerelease": false,
        "published_at": "2026-11-02T12:00:00Z"
    }]);
    let http = ScriptedHttp::new(vec![
        answer(200, &releases, "\"r\"", false),
        answer(
            200,
            &json!({"id": 9, "name": "cli", "html_url": "https://github.com/lemonfiber/cli"}),
            "",
            false,
        ),
    ]);
    let cli = member("9", "cli");
    let polled =
        pollster::block_on(source.polled(&http, &[(&cli, Some(&cursor))], &Rounded::Unread));
    let Ok(Some(found)) = &polled[0] else {
        panic!("the member's new release is found: {polled:?}");
    };
    assert_eq!(found.packages, cursor.packages);
    assert_eq!(
        found.signals.get(&Signal::Package),
        Some(&Validators::default())
    );
}

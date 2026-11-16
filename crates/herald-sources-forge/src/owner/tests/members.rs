//! An owner's member file, project templates and GraphQL batches.

use super::*;

fn raw(body: &str, etag: &str) -> Response {
    Response {
        status: 200,
        headers: vec![("ETag".to_owned(), etag.to_owned())],
        body: body.as_bytes().to_vec(),
    }
}

fn filed(forge: Forge, reference: Option<&str>) -> Owned {
    owned(
        forge,
        &SourceOptions {
            members: Some(Members::Repository {
                repository: "lemonfiber/spec".to_owned(),
                path: "30-repos/repos list.toml".to_owned(),
                reference: reference.map(ToOwned::to_owned),
            }),
            tables: vec!["repo".to_owned()],
            ..SourceOptions::default()
        },
    )
}

const FILE: &str = "[[repo]]\nname = \"herald\"\n[[repo]]\nname = \"Gone\"\n";

#[test]
fn a_member_file_beside_the_config_takes_only_what_it_names_and_names_what_the_owner_lacks() {
    let http = Scripted::new(vec![answer(
        200,
        &json!([github(1, "herald"), github(2, "plugin-a")]),
        "\"p1\"",
        false,
    )]);
    let source = Owned::new(
        Forge::GitHub,
        (
            "lemonfiber",
            Some(&["HERALD".to_owned(), "gone".to_owned()]),
        ),
        &SourceOptions::default(),
        (None, 0),
    );
    let expanded = listed(&source, &http, None).unwrap();
    assert_eq!(names(&expanded), ["herald"]);
    let Expanded::Listed { missing, file, .. } = expanded else {
        panic!("a first listing is read whole");
    };
    assert_eq!(missing, ["gone"]);
    assert_eq!(file, None);
    assert_ne!(
        source.fingerprint(),
        owned(Forge::GitHub, &SourceOptions::default()).fingerprint()
    );
}

#[test]
fn a_member_file_on_the_forge_is_asked_raw_at_its_reference_on_each_forge() {
    for (forge, reference, url, accept) in [
        (
            Forge::GitHub,
            Some("v1"),
            "https://api.github.com/repos/lemonfiber/spec/contents/30-repos/repos%20list.toml?ref=v1",
            herald_wire::github::RAW,
        ),
        (
            Forge::GitHub,
            None,
            "https://api.github.com/repos/lemonfiber/spec/contents/30-repos/repos%20list.toml",
            herald_wire::github::RAW,
        ),
        (
            Forge::GitLab,
            None,
            "https://gitlab.com/api/v4/projects/lemonfiber%2Fspec/repository/files/30-repos%2Frepos%20list.toml/raw?ref=HEAD",
            herald_core::media::JSON,
        ),
        (
            Forge::Forgejo,
            Some("main"),
            "https://codeberg.org/api/v1/repos/lemonfiber/spec/raw/30-repos/repos%20list.toml?ref=main",
            herald_core::media::JSON,
        ),
    ] {
        let source = filed(forge, reference);
        let request = source.file.as_ref().unwrap().origin.file_request(
            ("30-repos/repos list.toml", reference),
            source.token.as_ref(),
            Some("\"f1\""),
        );
        assert_eq!(request.url.redacted(), url);
        let headers: Vec<(&str, String)> = request
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.redacted()))
            .collect();
        assert_eq!(
            headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("accept"))
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            [accept]
        );
        assert!(headers.contains(&("If-None-Match", "\"f1\"".to_owned())));
    }
}

#[test]
fn a_member_file_on_the_forge_that_changed_reads_the_listing_whole() {
    let http = Scripted::new(vec![
        raw(FILE, "\"f2\""),
        answer(
            200,
            &json!([github(1, "herald"), github(2, "web")]),
            "\"p1\"",
            false,
        ),
    ]);
    let source = filed(Forge::GitHub, None);
    let known = Listing {
        file: Some("\"f1\"".to_owned()),
        events: None,
        ..known(&["\"p1\""])
    };
    let known = Listing {
        fingerprint: source.fingerprint(),
        ..known
    };
    let expanded = listed(&source, &http, Some(&known)).unwrap();
    assert_eq!(names(&expanded), ["herald"]);
    let Expanded::Listed { missing, file, .. } = expanded else {
        panic!("a changed member file reads the listing whole");
    };
    assert_eq!(missing, ["Gone"]);
    assert_eq!(file.as_deref(), Some("\"f2\""));
    let asked = http.asked();
    assert_eq!(asked[0].1.as_deref(), Some("\"f1\""));
    assert_eq!(asked[1].1, None);
}

#[test]
fn a_member_file_unchanged_is_read_again_only_where_the_listing_changed() {
    let source = filed(Forge::GitHub, None);
    let known = Listing {
        file: Some("\"f1\"".to_owned()),
        events: None,
        fingerprint: source.fingerprint(),
        ..known(&["\"p1\""])
    };
    let still = Scripted::new(vec![
        unchanged(),
        unchanged(),
        answer(200, &json!([]), "", false),
    ]);
    assert_eq!(
        listed(&source, &still, Some(&known)),
        Ok(Expanded::Unchanged)
    );
    let moved = Scripted::new(vec![
        unchanged(),
        answer(
            200,
            &json!([github(1, "herald"), github(3, "new")]),
            "\"p2\"",
            false,
        ),
        answer(
            200,
            &json!([github(1, "herald"), github(3, "new")]),
            "\"p2\"",
            false,
        ),
        raw(FILE, "\"f1\""),
    ]);
    let expanded = listed(&source, &moved, Some(&known)).unwrap();
    assert_eq!(names(&expanded), ["herald"]);
    assert!(matches!(
        expanded,
        Expanded::Listed { file: Some(ref etag), .. } if etag == "\"f1\""
    ));
    let asked = moved.asked();
    assert_eq!(asked.len(), 4);
    assert!(asked[3].0.contains("/contents/"));
    assert_eq!(asked[3].1, None);
}

#[test]
fn a_member_file_the_forge_does_not_serve_or_that_cannot_be_read_fails_the_listing() {
    for answer in [
        Response {
            status: 404,
            headers: Vec::new(),
            body: b"{}".to_vec(),
        },
        raw("not = [toml", "\"f\""),
    ] {
        let http = Scripted::new(vec![answer]);
        let failed = listed(&filed(Forge::GitHub, None), &http, None);
        assert!(
            matches!(failed, Err(Unlisted::Failed(Unpolled::Unreadable(_)))),
            "{failed:?}"
        );
        assert_eq!(http.asked().len(), 1);
    }
    let silent = Scripted::new(Vec::new());
    assert!(matches!(
        listed(&filed(Forge::GitHub, None), &silent, None),
        Err(Unlisted::Failed(Unpolled::Unreachable(_)))
    ));
}

fn cli() -> Listed {
    Listed {
        id: "9".to_owned(),
        name: "cli".to_owned(),
        path: "lemonfiber/cli".to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
    }
}

fn templated(project: &str) -> Owned {
    owned(
        Forge::GitHub,
        &SourceOptions {
            project: Some(project.to_owned()),
            ..SourceOptions::default()
        },
    )
}

fn released() -> Scripted {
    Scripted::new(vec![
        Response {
            status: 200,
            headers: Vec::new(),
            body: include_bytes!("../../../fixtures/github/releases.json").to_vec(),
        },
        Response {
            status: 200,
            headers: Vec::new(),
            body: include_bytes!("../../../fixtures/github/repository.json").to_vec(),
        },
    ])
}

#[test]
fn a_member_s_events_are_announced_as_the_project_its_template_names() {
    let source = templated("{{ repository.owner }}-{{ repository.name }}");
    assert_eq!(source.project(&cli()), Ok("lemonfiber-cli".to_owned()));
    let polled = pollster::block_on(source.member(&cli()).polled(&released(), None))
        .unwrap()
        .unwrap();
    assert_ne!(polled.events, Vec::new());
    assert!(
        polled
            .events
            .iter()
            .all(|event| event.project() == "lemonfiber-cli")
    );
    let plain = owned(Forge::GitHub, &SourceOptions::default());
    assert_eq!(plain.project(&cli()), Ok("cli".to_owned()));
    let untouched = pollster::block_on(plain.member(&cli()).polled(&released(), None))
        .unwrap()
        .unwrap();
    assert!(
        untouched
            .events
            .iter()
            .all(|event| event.project() == "neovim")
    );
}

#[test]
fn a_member_its_template_names_no_project_for_is_not_read() {
    let source = templated("{{ repository.properties.team[0] }}");
    let Err(reason) = source.project(&cli()) else {
        panic!("a member without the property has no project");
    };
    assert!(reason.starts_with("template project: "), "{reason}");
    assert_eq!(
        pollster::block_on(source.member(&cli()).polled(&released(), None)),
        Err(Unpolled::Unreadable(reason))
    );
}

#[test]
fn the_narrowest_override_naming_a_member_names_its_project() {
    let source = owned(
        Forge::GitHub,
        &SourceOptions {
            project: Some("source-{{ repository.name }}".to_owned()),
            repositories: BTreeMap::from([
                (
                    "c*".to_owned(),
                    crate::Override {
                        project: Some("wide-{{ repository.name }}".to_owned()),
                        ..crate::Override::default()
                    },
                ),
                (
                    "cli".to_owned(),
                    crate::Override {
                        project: Some("narrow".to_owned()),
                        ..crate::Override::default()
                    },
                ),
            ]),
            ..SourceOptions::default()
        },
    );
    assert_eq!(source.project(&cli()), Ok("narrow".to_owned()));
    let cat = Listed {
        name: "cat".to_owned(),
        ..cli()
    };
    assert_eq!(source.project(&cat), Ok("wide-cat".to_owned()));
    let web = Listed {
        name: "web".to_owned(),
        ..cli()
    };
    assert_eq!(source.project(&web), Ok("source-web".to_owned()));
    let polled = pollster::block_on(source.member(&cat).polled(&released(), None))
        .unwrap()
        .unwrap();
    assert!(
        polled
            .events
            .iter()
            .all(|event| event.project() == "wide-cat")
    );
}

#[test]
fn a_github_owner_with_a_token_polls_its_members_together_through_graphql() {
    let source = owned(
        Forge::GitHub,
        &SourceOptions {
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            graphql_batch: NonZeroUsize::new(2),
            project: Some("p-{{ repository.name }}".to_owned()),
            ..SourceOptions::default()
        },
    );
    assert_eq!(source.batch().get(), 2);
    let releases = json!({ "databaseId": 9, "releases": { "nodes": [{
        "tagName": "v1",
        "url": "https://github.com/lemonfiber/cli/releases/tag/v1",
        "isDraft": false,
        "publishedAt": "2026-11-01T12:00:00Z"
    }] } });
    let http = Scripted::new(vec![
        answer(200, &json!({ "data": { "m0": releases } }), "", false),
        answer(200, &json!({ "data": { "m0": releases } }), "", false),
    ]);
    let member = cli();
    let answered = pollster::block_on(source.polled(&http, &[(&member, None)], &Rounded::Nothing));
    let Ok(Some(polled)) = &answered[0] else {
        panic!("a member never polled is read in full: {answered:?}");
    };
    assert_eq!(polled.events[0].project(), "p-cli");
    assert_eq!(http.asked()[0].0, "https://api.github.com/graphql");
    let rest = owned(Forge::GitHub, &SourceOptions::default());
    assert_eq!(rest.batch(), NonZeroUsize::MIN);
    let unchanged = Scripted::new(vec![unchanged()]);
    assert_eq!(
        pollster::block_on(rest.polled(&unchanged, &[(&member, None)], &Rounded::Nothing)),
        [Ok(None)]
    );
    assert!(
        unchanged.asked()[0]
            .0
            .ends_with("/repos/lemonfiber/cli/releases?per_page=30")
    );
    let enterprise = owned(
        Forge::GitHub,
        &SourceOptions {
            api: Some("https://ghe.example/api/v3".to_owned()),
            transport: Some(crate::Transport::Graphql),
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            ..SourceOptions::default()
        },
    );
    assert_eq!(
        enterprise.graphql.as_deref(),
        Some("https://ghe.example/api/graphql")
    );
    assert_eq!(enterprise.batch(), crate::GRAPHQL_BATCH);
    let named = owned(
        Forge::GitHub,
        &SourceOptions {
            graphql: Some("https://api.github.com/graphql-beta".to_owned()),
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            ..SourceOptions::default()
        },
    );
    assert_eq!(
        named.graphql.as_deref(),
        Some("https://api.github.com/graphql-beta")
    );
    let gitlab = owned(
        Forge::GitLab,
        &SourceOptions {
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            ..SourceOptions::default()
        },
    );
    assert_eq!(
        gitlab.graphql.as_deref(),
        Some("https://gitlab.com/api/graphql")
    );
}

#[test]
fn a_gitlab_owner_polls_through_graphql_only_where_it_takes_releases_alone_and_forgejo_never() {
    let tagged = owned(
        Forge::GitLab,
        &SourceOptions {
            signals: Some(vec![Signal::Release, Signal::Tag]),
            ..SourceOptions::default()
        },
    );
    assert_eq!(tagged.graphql, None);
    let forgejo = owned(
        Forge::Forgejo,
        &SourceOptions {
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            ..SourceOptions::default()
        },
    );
    assert_eq!(forgejo.graphql, None);
}

#[test]
fn only_a_github_owner_following_its_events_or_a_forgejo_owner_following_its_feed_reads_them() {
    let following = |forge: Forge, transport: crate::Transport| {
        owned(
            forge,
            &SourceOptions {
                transport: Some(transport),
                ..SourceOptions::default()
            },
        )
    };
    let http = Scripted::new(vec![answer(200, &json!([]), "\"e1\"", false)]);
    let read = pollster::block_on(following(Forge::GitHub, crate::Transport::Events).changed(
        &http,
        Owner::Organization,
        None,
    ));
    assert!(
        matches!(read, Some(Ok(herald_kinds::Changed::Gap(_)))),
        "{read:?}"
    );
    assert!(http.asked()[0].0.contains("/orgs/lemonfiber/events?"));
    let fed = Scripted::new(vec![answer(200, &json!([]), "", false)]);
    let paged = owned(
        Forge::Forgejo,
        &SourceOptions {
            transport: Some(crate::Transport::Feed),
            feed_page: std::num::NonZeroUsize::new(7),
            ..SourceOptions::default()
        },
    );
    let read = pollster::block_on(paged.changed(&fed, Owner::User, None));
    assert!(
        matches!(read, Some(Ok(herald_kinds::Changed::Gap(_)))),
        "{read:?}"
    );
    assert!(
        fed.asked()[0]
            .0
            .contains("/users/lemonfiber/activities/feeds?")
    );
    let silent = Scripted::new(Vec::new());
    for (forge, transport) in [
        (Forge::GitHub, crate::Transport::Rest),
        (Forge::GitHub, crate::Transport::Feed),
        (Forge::GitLab, crate::Transport::Events),
        (Forge::Forgejo, crate::Transport::Events),
    ] {
        assert_eq!(
            pollster::block_on(following(forge, transport).changed(&silent, Owner::User, None)),
            None
        );
    }
    assert_eq!(silent.asked(), Vec::new());
}

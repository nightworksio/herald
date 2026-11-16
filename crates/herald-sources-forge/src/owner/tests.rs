use herald_ports::Secret;
use serde_json::{Value, json};

use super::*;
use crate::scripted::Scripted;

fn answer(status: u16, body: &Value, etag: &str, next: bool) -> Response {
    let mut headers = vec![("ETag".to_owned(), etag.to_owned())];
    if next {
        headers.push((
            "Link".to_owned(),
            "<https://api.github.com/orgs/o/repos?page=2>; rel=\"next\", <https://api.github.com/orgs/o/repos?page=9>; rel=\"last\"".to_owned(),
        ));
    }
    Response {
        status,
        headers,
        body: body.to_string().into_bytes(),
    }
}

fn unchanged() -> Response {
    Response {
        status: 304,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn github(id: u64, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "full_name": format!("lemonfiber/{name}"),
        "visibility": "public",
        "topics": ["media"],
        "custom_properties": { "team": "media", "tier": ["core", "edge"], "unset": null },
        "language": "Rust"
    })
}

fn owned(forge: Forge, options: &SourceOptions) -> Owned {
    Owned::new(
        forge,
        ("lemonfiber", None),
        options,
        (Some(Token::Named(Secret::new("TOKEN", "t"))), 0),
    )
}

fn listed(owned: &Owned, http: &Scripted, known: Option<&Listing>) -> Result<Expanded, Unlisted> {
    pollster::block_on(owned.listed(http, known))
}

fn names(expanded: &Expanded) -> Vec<&str> {
    match expanded {
        Expanded::Listed { members, .. } => {
            members.iter().map(|member| member.name.as_str()).collect()
        }
        Expanded::Unchanged => Vec::new(),
    }
}

fn known(pages: &[&str]) -> Listing {
    Listing {
        owner: Owner::Organization,
        pages: pages.iter().map(|page| Some((*page).to_owned())).collect(),
        first: jiff::Timestamp::UNIX_EPOCH,
        listed: jiff::Timestamp::UNIX_EPOCH,
        fingerprint: owned(Forge::GitHub, &SourceOptions::default()).fingerprint(),
        file: None,
        events: None,
    }
}

#[test]
fn each_forge_lists_an_organization_and_a_user_at_its_own_api() {
    let options = SourceOptions {
        listed_page: NonZeroUsize::new(7),
        subgroups: Some(false),
        api: Some("https://forge.example/api/".to_owned()),
        ..SourceOptions::default()
    };
    let url = |forge, owner| {
        owned(forge, &options)
            .page_request(owner, 2, None)
            .url
            .redacted()
    };
    assert_eq!(
        url(Forge::GitHub, Owner::Organization),
        "https://forge.example/api/orgs/lemonfiber/repos?type=all&sort=full_name&direction=asc&per_page=7&page=2"
    );
    assert_eq!(
        url(Forge::GitHub, Owner::User),
        "https://forge.example/api/users/lemonfiber/repos?type=owner&sort=full_name&direction=asc&per_page=7&page=2"
    );
    assert_eq!(
        url(Forge::GitLab, Owner::Organization),
        "https://forge.example/api/groups/lemonfiber/projects?include_subgroups=false&with_shared=false&order_by=id&sort=asc&per_page=7&page=2"
    );
    assert_eq!(
        url(Forge::GitLab, Owner::User),
        "https://forge.example/api/users/lemonfiber/projects?order_by=id&sort=asc&per_page=7&page=2"
    );
    assert_eq!(
        url(Forge::Forgejo, Owner::Organization),
        "https://forge.example/api/orgs/lemonfiber/repos?limit=7&page=2"
    );
    assert_eq!(
        url(Forge::Forgejo, Owner::User),
        "https://forge.example/api/users/lemonfiber/repos?limit=7&page=2"
    );
    let defaults = |forge| {
        owned(forge, &SourceOptions::default()).page_request(Owner::Organization, 1, Some("\"e\""))
    };
    let github = defaults(Forge::GitHub);
    assert!(
        github
            .url
            .redacted()
            .starts_with("https://api.github.com/orgs/")
    );
    assert!(github.url.redacted().contains("per_page=100&"));
    assert!(
        github
            .headers
            .iter()
            .any(|(name, value)| name == "If-None-Match" && value.redacted() == "\"e\"")
    );
    assert!(defaults(Forge::GitLab).url.redacted().starts_with(
        "https://gitlab.com/api/v4/groups/lemonfiber/projects?include_subgroups=true&"
    ));
    assert!(
        defaults(Forge::Forgejo)
            .url
            .redacted()
            .ends_with("limit=50&page=1")
    );
}

#[test]
fn a_page_is_read_as_each_forge_describes_its_repositories() {
    let page = |forge, body: Value| {
        owned(forge, &SourceOptions::default()).page(&Response {
            status: 200,
            headers: Vec::new(),
            body: body.to_string().into_bytes(),
        })
    };
    let read = page(
        Forge::GitHub,
        json!([github(1, "herald"), {
            "id": 2, "name": "old", "full_name": "lemonfiber/old", "private": true,
            "archived": true, "fork": true, "is_template": true
        }, {
            "id": 3, "name": "open", "full_name": "lemonfiber/open"
        }]),
    )
    .unwrap();
    assert_eq!(
        read[0],
        Listed {
            id: "1".to_owned(),
            name: "herald".to_owned(),
            path: "lemonfiber/herald".to_owned(),
            topics: vec!["media".to_owned()],
            properties: BTreeMap::from([
                ("team".to_owned(), vec!["media".to_owned()]),
                (
                    "tier".to_owned(),
                    vec!["core".to_owned(), "edge".to_owned()]
                ),
            ]),
            visibility: Visibility::Public,
            archived: false,
            fork: false,
            template: false,
            language: Some("Rust".to_owned()),
        }
    );
    assert_eq!(
        (
            read[1].visibility,
            read[1].archived,
            read[1].fork,
            read[1].template
        ),
        (Visibility::Private, true, true, true)
    );
    assert_eq!(read[2].visibility, Visibility::Private);
    let gitlab = page(
        Forge::GitLab,
        json!([{
            "id": 9, "path": "cli", "path_with_namespace": "lemonfiber/tools/cli",
            "topics": ["t"], "visibility": "internal", "archived": true,
            "forked_from_project": { "id": 1 }
        }, {
            "id": 10, "path": "web", "path_with_namespace": "lemonfiber/web", "visibility": "private"
        }]),
    )
    .unwrap();
    assert_eq!(
        gitlab[0],
        Listed {
            id: "9".to_owned(),
            name: "cli".to_owned(),
            path: "lemonfiber/tools/cli".to_owned(),
            topics: vec!["t".to_owned()],
            properties: BTreeMap::new(),
            visibility: Visibility::Internal,
            archived: true,
            fork: true,
            template: false,
            language: None,
        }
    );
    assert!(!gitlab[1].fork);
    let forgejo = page(
        Forge::Forgejo,
        json!([
            { "id": 4, "name": "a", "full_name": "lemonfiber/a", "private": true, "internal": true,
              "template": true, "language": "", "topics": ["x"] },
            { "id": 5, "name": "b", "full_name": "lemonfiber/b", "private": false, "internal": true, "language": "Go",
              "archived": true, "fork": true },
            { "id": 6, "name": "c", "full_name": "lemonfiber/c", "private": false }
        ]),
    )
    .unwrap();
    assert_eq!(
        forgejo
            .iter()
            .map(|listed| (listed.visibility, listed.language.clone()))
            .collect::<Vec<_>>(),
        [
            (Visibility::Private, None),
            (Visibility::Internal, Some("Go".to_owned())),
            (Visibility::Public, None)
        ]
    );
    assert!(forgejo[0].template && forgejo[1].archived && forgejo[1].fork);
    assert_eq!(forgejo[0].topics, ["x"]);
    assert!(page(Forge::GitHub, json!({ "message": "Not Found" })).is_err());
}

#[test]
fn a_visibility_missing_or_unknown_is_read_as_private() {
    let visibilities = |forge, entries: Value| {
        owned(forge, &SourceOptions::default())
            .page(&Response {
                status: 200,
                headers: Vec::new(),
                body: entries.to_string().into_bytes(),
            })
            .unwrap()
            .into_iter()
            .map(|listed| listed.visibility)
            .collect::<Vec<_>>()
    };
    let github = |extra: Value| {
        let mut entry = json!({ "id": 1, "name": "a", "full_name": "o/a" });
        if let (Some(entry), Some(extra)) = (entry.as_object_mut(), extra.as_object()) {
            entry.extend(extra.clone());
        }
        entry
    };
    assert_eq!(
        visibilities(
            Forge::GitHub,
            json!([
                github(json!({ "visibility": "public", "private": true })),
                github(json!({ "visibility": "internal" })),
                github(json!({ "visibility": "private" })),
                github(json!({ "visibility": "secret" })),
                github(json!({ "private": false })),
                github(json!({ "private": true })),
                github(json!({}))
            ])
        ),
        [
            Visibility::Public,
            Visibility::Internal,
            Visibility::Private,
            Visibility::Private,
            Visibility::Public,
            Visibility::Private,
            Visibility::Private
        ]
    );
    let gitlab = |extra: Value| {
        let mut entry = json!({ "id": 1, "path": "a", "path_with_namespace": "g/a" });
        if let (Some(entry), Some(extra)) = (entry.as_object_mut(), extra.as_object()) {
            entry.extend(extra.clone());
        }
        entry
    };
    assert_eq!(
        visibilities(
            Forge::GitLab,
            json!([
                gitlab(json!({ "visibility": "public", "visibility_level": 0 })),
                gitlab(json!({ "visibility_level": 20 })),
                gitlab(json!({ "visibility_level": 10 })),
                gitlab(json!({ "visibility_level": 0 })),
                gitlab(json!({}))
            ])
        ),
        [
            Visibility::Public,
            Visibility::Public,
            Visibility::Internal,
            Visibility::Private,
            Visibility::Private
        ]
    );
    assert_eq!(
        visibilities(
            Forge::Forgejo,
            json!([
                { "id": 1, "name": "a", "full_name": "o/a" },
                { "id": 2, "name": "b", "full_name": "o/b", "internal": true },
                { "id": 3, "name": "c", "full_name": "o/c", "private": false, "internal": true }
            ])
        ),
        [
            Visibility::Private,
            Visibility::Private,
            Visibility::Internal
        ]
    );
}

#[test]
fn a_listing_reads_every_page_the_forge_names_and_keeps_what_the_selection_takes() {
    let http = Scripted::new(vec![
        answer(
            200,
            &json!([github(1, "herald"), github(2, "plugin-a")]),
            "\"p1\"",
            true,
        ),
        answer(200, &json!([github(3, "plugin-b")]), "\"p2\"", false),
    ]);
    let source = owned(
        Forge::GitHub,
        &SourceOptions {
            include: vec!["plugin-*".to_owned()],
            ..SourceOptions::default()
        },
    );
    let expanded = listed(&source, &http, None).unwrap();
    assert_eq!(names(&expanded), ["plugin-a", "plugin-b"]);
    assert_eq!(
        expanded,
        Expanded::Listed {
            members: match &expanded {
                Expanded::Listed { members, .. } => members.clone(),
                Expanded::Unchanged => Vec::new(),
            },
            owner: Owner::Organization,
            pages: vec![Some("\"p1\"".to_owned()), Some("\"p2\"".to_owned())],
            file: None,
            missing: Vec::new(),
        }
    );
    assert_eq!(http.asked().len(), 2);
    assert!(http.asked()[1].0.ends_with("&page=2"));
    assert_eq!(source.api(), "https://api.github.com");
    assert_eq!(source.keep_members(), crate::KEEP_MEMBERS);
    assert_eq!(
        source.remembered(),
        SourceOptions::default().reading().remembered
    );
    assert!(source.remembered() > 1);
}

#[test]
fn an_owner_no_organization_names_is_listed_as_a_user() {
    let http = Scripted::new(vec![
        answer(404, &json!({ "message": "Not Found" }), "", false),
        answer(200, &json!([github(1, "herald")]), "\"u1\"", false),
    ]);
    let expanded = listed(
        &owned(Forge::GitHub, &SourceOptions::default()),
        &http,
        None,
    )
    .unwrap();
    assert!(matches!(
        expanded,
        Expanded::Listed {
            owner: Owner::User,
            ..
        }
    ));
    assert!(http.asked()[1].0.contains("/users/lemonfiber/repos?"));
    let gone = Scripted::new(vec![
        answer(404, &json!({}), "", false),
        answer(404, &json!({}), "", false),
    ]);
    assert!(matches!(
        listed(
            &owned(Forge::GitHub, &SourceOptions::default()),
            &gone,
            None
        ),
        Err(Unlisted::Failed(Unpolled::Unreadable(_)))
    ));
    let known_gone = Scripted::new(vec![answer(404, &json!({}), "", false)]);
    assert!(matches!(
        listed(
            &owned(Forge::GitHub, &SourceOptions::default()),
            &known_gone,
            Some(&Listing {
                pages: Vec::new(),
                ..known(&[])
            })
        ),
        Err(Unlisted::Failed(_))
    ));
    assert_eq!(known_gone.asked().len(), 1);
}

#[test]
fn a_listing_whose_every_page_is_unchanged_and_that_grew_no_page_is_unchanged() {
    let http = Scripted::new(vec![
        unchanged(),
        unchanged(),
        answer(200, &json!([]), "", false),
    ]);
    let source = owned(Forge::GitHub, &SourceOptions::default());
    assert_eq!(
        listed(&source, &http, Some(&known(&["\"p1\"", "\"p2\""]))),
        Ok(Expanded::Unchanged)
    );
    let asked = http.asked();
    assert_eq!(asked[0].1.as_deref(), Some("\"p1\""));
    assert_eq!(asked[1].1.as_deref(), Some("\"p2\""));
    assert_eq!(asked[2].1, None);
    assert!(asked[2].0.ends_with("&page=3"));
}

#[test]
fn a_listing_with_a_changed_or_a_new_page_is_read_again_from_its_first_page() {
    let grown = Scripted::new(vec![
        unchanged(),
        answer(200, &json!([github(2, "new")]), "\"n\"", false),
        answer(200, &json!([github(1, "herald")]), "\"p1\"", true),
        answer(200, &json!([github(2, "new")]), "\"n\"", false),
    ]);
    let source = owned(Forge::GitHub, &SourceOptions::default());
    let expanded = listed(&source, &grown, Some(&known(&["\"p1\""]))).unwrap();
    assert_eq!(names(&expanded), ["herald", "new"]);
    assert_eq!(grown.asked()[2].1, None);
    let changed = Scripted::new(vec![
        answer(200, &json!([github(1, "renamed")]), "\"r\"", false),
        answer(200, &json!([github(1, "renamed")]), "\"r\"", false),
    ]);
    let expanded = listed(&source, &changed, Some(&known(&["\"p1\"", "\"p2\""]))).unwrap();
    assert_eq!(names(&expanded), ["renamed"]);
    assert_eq!(changed.asked().len(), 2);
}

#[test]
fn a_listing_over_a_cap_refuses_the_source_naming_the_setting() {
    let page = json!([github(1, "a"), github(2, "b"), github(3, "c")]);
    let http = Scripted::new(vec![answer(200, &page, "", true)]);
    let capped = owned(
        Forge::GitHub,
        &SourceOptions {
            most_listed: NonZeroUsize::new(2),
            ..SourceOptions::default()
        },
    );
    assert_eq!(
        listed(&capped, &http, None),
        Err(Unlisted::Over(
            "lemonfiber lists more than 2 repositories, most_listed; raise it, or name a narrower owner"
                .to_owned()
        ))
    );
    assert_eq!(http.asked().len(), 1);
    let exactly = Scripted::new(vec![answer(200, &page, "", false)]);
    let at_cap = owned(
        Forge::GitHub,
        &SourceOptions {
            most_listed: NonZeroUsize::new(3),
            most_members: NonZeroUsize::new(2),
            ..SourceOptions::default()
        },
    );
    assert_eq!(
        listed(&at_cap, &exactly, None),
        Err(Unlisted::Over(
            "3 repositories of lemonfiber are taken, more than most_members, 2; raise it, or narrow the filters"
                .to_owned()
        ))
    );
    let fits = Scripted::new(vec![answer(200, &page, "", false)]);
    let roomy = owned(
        Forge::GitHub,
        &SourceOptions {
            most_listed: NonZeroUsize::new(3),
            most_members: NonZeroUsize::new(3),
            ..SourceOptions::default()
        },
    );
    assert_eq!(
        names(&listed(&roomy, &fits, None).unwrap()),
        ["a", "b", "c"]
    );
}

#[test]
fn a_listing_that_fails_or_names_a_next_page_after_an_empty_one_stops() {
    let source = owned(Forge::GitHub, &SourceOptions::default());
    let unreachable = Scripted::new(Vec::new());
    assert!(matches!(
        listed(&source, &unreachable, None),
        Err(Unlisted::Failed(Unpolled::Unreachable(_)))
    ));
    let endless = Scripted::new(vec![answer(200, &json!([]), "", true)]);
    assert_eq!(
        names(&listed(&source, &endless, None).unwrap()),
        Vec::<&str>::new()
    );
    assert_eq!(endless.asked().len(), 1);
    let partway = Scripted::new(vec![
        answer(200, &json!([github(1, "a")]), "", true),
        answer(500, &json!({}), "", false),
    ]);
    assert!(matches!(
        listed(&source, &partway, None),
        Err(Unlisted::Failed(Unpolled::Unreadable(_)))
    ));
}

#[test]
fn a_member_is_polled_at_its_own_path_with_the_source_s_token() {
    let source = owned(Forge::GitLab, &SourceOptions::default());
    let poll = source.member(&Listed {
        id: "9".to_owned(),
        name: "cli".to_owned(),
        path: "lemonfiber/tools/cli".to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
    });
    assert_eq!(poll.api(), "https://gitlab.com/api/v4");
    let http = Scripted::new(vec![unchanged()]);
    assert_eq!(pollster::block_on(poll.polled(&http, None)), Ok(None));
    assert_eq!(
        http.asked()[0].0,
        "https://gitlab.com/api/v4/projects/lemonfiber%2Ftools%2Fcli/releases?per_page=30"
    );
}

#[test]
fn a_link_names_a_next_page_only_with_rel_next() {
    let linked = |link: &str| Response {
        status: 200,
        headers: vec![("Link".to_owned(), link.to_owned())],
        body: Vec::new(),
    };
    assert!(next(&linked("<a>; rel=\"next\"")));
    assert!(!next(&linked("<a>; rel=\"last\", <b>; rel=\"prev\"")));
    assert!(!next(&linked("rel=\"next\"")));
    assert!(!next(&Response {
        status: 200,
        headers: Vec::new(),
        body: Vec::new(),
    }));
}

#[test]
fn a_listing_whose_pages_named_no_etag_is_read_whole_at_once() {
    let http = Scripted::new(vec![answer(200, &json!([github(1, "a")]), "", false)]);
    let source = owned(Forge::GitHub, &SourceOptions::default());
    let last = Listing {
        pages: vec![Some("\"p1\"".to_owned()), None],
        ..known(&[])
    };
    assert_eq!(names(&listed(&source, &http, Some(&last)).unwrap()), ["a"]);
    assert_eq!(
        http.asked(),
        [(
            source
                .page_request(Owner::Organization, 1, None)
                .url
                .redacted(),
            None
        )]
    );
}

#[test]
fn a_listing_kept_for_another_selection_is_read_whole() {
    let http = Scripted::new(vec![answer(200, &json!([github(1, "a")]), "\"n\"", false)]);
    let narrowed = owned(
        Forge::GitHub,
        &SourceOptions {
            exclude: vec!["b".to_owned()],
            ..SourceOptions::default()
        },
    );
    assert_ne!(narrowed.fingerprint(), known(&[]).fingerprint);
    assert_eq!(
        names(&listed(&narrowed, &http, Some(&known(&["\"p1\""]))).unwrap()),
        ["a"]
    );
    assert_eq!(http.asked()[0].1, None);
    assert!(narrowed.takes(&Listed::from(
        serde_json::from_value::<GitHub>(github(1, "a")).unwrap()
    )));
    assert!(!narrowed.takes(&Listed::from(
        serde_json::from_value::<GitHub>(github(2, "b")).unwrap()
    )));
}

mod members;
mod packages;

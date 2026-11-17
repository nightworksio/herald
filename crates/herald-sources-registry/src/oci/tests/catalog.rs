use std::num::NonZeroUsize;
use std::sync::Arc;

use herald_core::{Listed, Visibility};
use herald_kinds::{Expand, Expanded, Probed, Unlisted, Unpolled};
use herald_testing::ScriptedHttp;
use jiff::SignedDuration;

use super::super::catalog::Catalogued;
use super::super::session::{Failure, Reached};
use super::*;

const REGISTRY: &str = "https://registry.example";

fn catalog(options: &OciOptions, token: Option<Secret>) -> Catalogued {
    Catalogued::new(Arc::new(Reached::new(options, token)), options)
}

fn names(names: &[&str]) -> Response {
    answer(
        200,
        &serde_json::json!({ "repositories": names }).to_string(),
    )
}

fn listed(catalog: &Catalogued, http: &ScriptedHttp) -> Result<Expanded, Unlisted> {
    pollster::block_on(catalog.listed(http, None))
}

fn members(expanded: Expanded) -> Vec<(String, String, Visibility)> {
    expanded
        .members()
        .into_iter()
        .map(|member| (member.path, member.name, member.visibility))
        .collect()
}

fn member(path: &str) -> Listed {
    Listed {
        id: path.to_owned(),
        name: path.to_owned(),
        path: path.to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

#[test]
fn a_catalog_is_listed_page_by_page_and_taken_as_its_selection_says() {
    let options = OciOptions {
        listed_page: NonZeroUsize::new(2),
        exclude: vec!["acme/old".to_owned()],
        ..catalogued(REGISTRY)
    };
    let source = catalog(&options, None);
    let http = ScriptedHttp::new(vec![
        names(&["acme/app", "acme/old"]),
        with(
            names(&["tool"]),
            "Link",
            r#"</v2/_catalog?last=tool&n=2>; rel="next""#,
        ),
        names(&[]),
    ]);
    let expanded = listed(&source, &http).unwrap();
    let Expanded::Listed {
        owner,
        pages,
        file,
        missing,
        ..
    } = &expanded
    else {
        panic!("the catalog is listed");
    };
    assert_eq!(*owner, herald_core::Owner::Organization);
    assert!(pages.is_empty() && file.is_none() && missing.is_empty());
    assert_eq!(
        members(expanded),
        [
            (
                "acme/app".to_owned(),
                "acme/app".to_owned(),
                Visibility::Public
            ),
            ("tool".to_owned(), "tool".to_owned(), Visibility::Public),
        ]
    );
    assert_eq!(
        http.asked()
            .into_iter()
            .map(|(url, _)| url)
            .collect::<Vec<_>>(),
        [
            "https://registry.example/v2/_catalog?n=2",
            "https://registry.example/v2/_catalog?n=2&last=acme%2Fold",
            "https://registry.example/v2/_catalog?n=2&last=tool",
        ]
    );
}

#[test]
fn a_catalog_read_with_a_token_takes_its_repositories_as_private_and_asks_for_catalog_scope() {
    let options = OciOptions {
        username: Some("me".to_owned()),
        token: Some(token()),
        ..catalogued(REGISTRY)
    };
    let source = catalog(&options, Some(read_token()));
    let http = ScriptedHttp::new(vec![
        with(
            answer(401, ""),
            "WWW-Authenticate",
            r#"Bearer realm="https://registry.example/token""#,
        ),
        answer(200, r#"{"token":"t"}"#),
        names(&["acme/app"]),
    ]);
    assert_eq!(
        members(listed(&source, &http).unwrap()),
        [(
            "acme/app".to_owned(),
            "acme/app".to_owned(),
            Visibility::Private
        )]
    );
    assert_eq!(
        http.requests()[1].url.expose(),
        "https://registry.example/token?scope=registry%3Acatalog%3A%2A"
    );
}

#[test]
fn a_catalog_over_its_caps_repeating_a_name_or_unanswered_refuses_the_round() {
    let capped = OciOptions {
        most_listed: NonZeroUsize::new(1),
        ..catalogued(REGISTRY)
    };
    assert_eq!(
        listed(
            &catalog(&capped, None),
            &ScriptedHttp::new(vec![names(&["a", "b"])])
        ),
        Err(Unlisted::Over(
            "the registry's catalog lists more than 1 repositories, most_listed; raise it"
                .to_owned()
        ))
    );
    let at_caps = OciOptions {
        most_listed: NonZeroUsize::new(2),
        most_members: NonZeroUsize::new(2),
        ..catalogued(REGISTRY)
    };
    assert_eq!(
        members(
            listed(
                &catalog(&at_caps, None),
                &ScriptedHttp::new(vec![names(&["a", "b"])])
            )
            .unwrap()
        )
        .len(),
        2
    );
    let kept = OciOptions {
        most_members: NonZeroUsize::new(1),
        ..catalogued(REGISTRY)
    };
    assert_eq!(
        listed(&catalog(&kept, None), &ScriptedHttp::new(vec![names(&["a", "b"])])),
        Err(Unlisted::Over(
            "2 repositories of the catalog are taken, more than most_members, 1; raise it, or narrow include and exclude".to_owned()
        ))
    );
    let paged = OciOptions {
        listed_page: NonZeroUsize::new(1),
        ..catalogued(REGISTRY)
    };
    assert_eq!(
        listed(
            &catalog(&paged, None),
            &ScriptedHttp::new(vec![names(&["a"]), names(&["a"])])
        ),
        Err(Unlisted::Failed(Unpolled::Unreadable(
            "the registry's catalog named \"a\" twice; it does not page its catalog by last"
                .to_owned()
        )))
    );
    let source = catalog(&catalogued(REGISTRY), None);
    for unanswered in [answer(404, ""), answer(200, "[]")] {
        assert!(matches!(
            listed(&source, &ScriptedHttp::new(vec![unanswered])),
            Err(Unlisted::Failed(Unpolled::Unreadable(_)))
        ));
    }
    assert!(matches!(
        listed(&source, &ScriptedHttp::new(Vec::new())),
        Err(Unlisted::Failed(Unpolled::Unreachable(_)))
    ));
}

#[test]
fn each_repository_is_polled_announced_and_named_as_a_repository_source_of_it_is() {
    let options = OciOptions {
        include: vec!["acme/*".to_owned()],
        keep_members: Some(herald_core::Duration(SignedDuration::from_hours(5))),
        ..catalogued("https://registry-1.docker.io")
    };
    let source = catalog(&options, None);
    assert_eq!(source.api(), "https://registry-1.docker.io");
    assert_eq!(source.bases().len(), 4);
    assert_eq!(source.owner_key(), "catalog");
    assert!(!source.pushed());
    assert_eq!(source.member_nouns(), ("repository", "repositories"));
    assert_eq!(source.keep_members(), SignedDuration::from_hours(5));
    assert_eq!(source.remembered(), 10_000);
    let app = member("acme/app");
    assert!(source.takes(&app));
    assert!(!source.takes(&member("other/app")));
    assert_eq!(source.project(&app), Ok("acme/app".to_owned()));
    assert_eq!(
        source.project(&member("library/caddy")),
        Ok("caddy".to_owned())
    );
    let origin = source.origin(&app);
    assert_eq!(
        (
            origin.forge.as_str(),
            origin.host.as_str(),
            origin.owner.as_str()
        ),
        ("oci", "docker.io", "acme")
    );
    assert_eq!(source.member(&app).api(), "https://registry-1.docker.io");
    assert_eq!(source.member(&app).remembered(), 10_000);
    assert_eq!(source.standing(&app), None);
}

#[test]
fn a_catalog_s_fingerprint_follows_its_registry_selection_visibility_and_page() {
    let base = catalogued(REGISTRY);
    let fingerprint = |options: &OciOptions| catalog(options, None).fingerprint();
    let first = fingerprint(&base);
    assert_eq!(first, fingerprint(&base));
    for changed in [
        OciOptions {
            registry: "https://other.example".to_owned(),
            ..base.clone()
        },
        OciOptions {
            include: vec!["a*".to_owned()],
            ..base.clone()
        },
        OciOptions {
            token: Some(token()),
            username: Some("me".to_owned()),
            ..base.clone()
        },
        OciOptions {
            listed_page: NonZeroUsize::new(5),
            ..base.clone()
        },
    ] {
        assert_ne!(fingerprint(&changed), first, "{changed:?}");
    }
}

#[test]
fn checking_a_catalog_checks_its_first_repository_and_nothing_where_it_lists_none() {
    let source = catalog(&catalogued(REGISTRY), None);
    let check = |answers: Vec<Response>| {
        let http = ScriptedHttp::new(answers);
        let probed = pollster::block_on(source.probed(&http));
        (
            probed,
            http.asked()
                .into_iter()
                .map(|(url, _)| url)
                .collect::<Vec<_>>(),
        )
    };
    let (probed, asked) = check(vec![
        names(&["acme/app"]),
        with(answer(200, r#"{"tags":["1.0"]}"#), "Date", DATE),
        answer(200, r#"{"config":{"digest":"sha256:c"}}"#),
        with(answer(307, ""), "Location", "https://cdn.example/b"),
    ]);
    assert_eq!(probed.len(), 1);
    assert_eq!(probed[0].key, "blobs");
    assert_eq!(
        asked[..2],
        [
            "https://registry.example/v2/_catalog?n=1",
            "https://registry.example/v2/acme/app/tags/list?n=1",
        ]
    );
    assert_eq!(check(vec![names(&[])]).0, Vec::<Probed>::new());
    assert_eq!(check(vec![answer(500, "")]).0, Vec::<Probed>::new());
    assert_eq!(check(Vec::new()).0, Vec::<Probed>::new());
}

#[test]
fn a_failure_unlists_the_catalog_as_its_poll_would_fail() {
    assert_eq!(
        Unlisted::from(Failure::at("catalog", "refused")),
        Unlisted::Failed(Unpolled::Unreadable("refused".to_owned()))
    );
}

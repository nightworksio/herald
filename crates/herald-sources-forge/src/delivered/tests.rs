use super::*;
use herald_core::Visibility;

#[test]
fn a_github_delivery_names_what_it_carries_and_leaves_the_rest_aside() {
    let payload = br#"{"repository":{"id":7,"name":"herald","full_name":"lemonfiber/herald","topics":["media"],"custom_properties":{"team":"media","tier":["core","edge"],"unset":null},"visibility":"internal","archived":true,"fork":false,"is_template":true,"language":"Rust"}}"#;
    let (id, described) = delivered(Forge::GitHub, "GitHub's release event", payload).unwrap();
    assert_eq!(id, "7");
    assert_eq!(
        described,
        Delivered {
            name: "herald".to_owned(),
            path: "lemonfiber/herald".to_owned(),
            topics: Some(vec!["media".to_owned()]),
            properties: Some(BTreeMap::from([
                ("team".to_owned(), vec!["media".to_owned()]),
                (
                    "tier".to_owned(),
                    vec!["core".to_owned(), "edge".to_owned()]
                ),
            ])),
            visibility: Some(Visibility::Internal),
            archived: Some(true),
            fork: Some(false),
            template: Some(true),
            language: Some("Rust".to_owned()),
        }
    );
    let bare = br#"{"repository":{"id":7,"name":"herald","full_name":"lemonfiber/herald"}}"#;
    let (_, bare) = delivered(Forge::Forgejo, "Forgejo's release event", bare).unwrap();
    assert_eq!(
        bare,
        Delivered {
            name: "herald".to_owned(),
            path: "lemonfiber/herald".to_owned(),
            ..Delivered::default()
        }
    );
}

#[test]
fn a_forgejo_delivery_s_flags_give_its_visibility_and_template() {
    let visibility = |flags: &str| {
        let payload =
            format!(r#"{{"repository":{{"id":1,"name":"a","full_name":"o/a",{flags}}}}}"#);
        delivered(
            Forge::Forgejo,
            "Forgejo's release event",
            payload.as_bytes(),
        )
        .unwrap()
        .1
    };
    assert_eq!(
        visibility(r#""private":false,"internal":true"#).visibility,
        Some(Visibility::Internal)
    );
    assert_eq!(
        visibility(r#""private":false"#).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        visibility(r#""private":true,"template":true"#),
        Delivered {
            name: "a".to_owned(),
            path: "o/a".to_owned(),
            visibility: Some(Visibility::Private),
            template: Some(true),
            ..Delivered::default()
        }
    );
}

#[test]
fn a_gitlab_delivery_names_its_project_by_its_path_s_last_name_and_level() {
    let payload = br#"{"project":{"id":3,"name":"Display Name","path_with_namespace":"group/sub/cli","visibility_level":0}}"#;
    assert_eq!(
        delivered(Forge::GitLab, "GitLab's release event", payload),
        Ok((
            "3".to_owned(),
            Delivered {
                name: "cli".to_owned(),
                path: "group/sub/cli".to_owned(),
                visibility: Some(Visibility::Private),
                ..Delivered::default()
            }
        ))
    );
    let unlevelled = br#"{"project":{"id":3,"path_with_namespace":"group/cli"}}"#;
    assert_eq!(
        delivered(Forge::GitLab, "GitLab's release event", unlevelled)
            .unwrap()
            .1
            .visibility,
        None
    );
    assert!(delivered(Forge::GitLab, "GitLab's release event", b"{}").is_err());
}

#[test]
fn a_repository_the_api_answers_with_is_the_origin_of_its_events() {
    let github = br#"{"id":7,"name":"herald","full_name":"lemonfiber/herald","topics":["media"],"custom_properties":{"team":"media"},"visibility":"internal"}"#;
    let origin = answered_origin(
        Forge::GitHub,
        ("github.com", "GitHub's repository", "x/y"),
        github,
    )
    .unwrap();
    assert_eq!(
        origin,
        Origin {
            forge: "github".to_owned(),
            host: "github.com".to_owned(),
            owner: "lemonfiber".to_owned(),
            repository: "lemonfiber/herald".to_owned(),
            id: "7".to_owned(),
            topics: vec!["media".to_owned()],
            properties: BTreeMap::from([("team".to_owned(), vec!["media".to_owned()])]),
            visibility: Visibility::Internal,
            areas: None,
            source: None,
        }
    );
    let forgejo = br#"{"id":8,"name":"gate","private":false,"internal":true}"#;
    let origin = answered_origin(
        Forge::Forgejo,
        ("codeberg.org", "Forgejo's repository", "nightworks/gate"),
        forgejo,
    )
    .unwrap();
    assert_eq!(origin.repository, "nightworks/gate");
    assert_eq!(origin.owner, "nightworks");
    assert_eq!(origin.visibility, Visibility::Internal);
    assert!(origin.topics.is_empty() && origin.properties.is_empty());
    let gitlab =
        br#"{"id":9,"path_with_namespace":"group/sub/cli","topics":["cli"],"visibility":"public"}"#;
    let origin = answered_origin(
        Forge::GitLab,
        ("gitlab.com", "GitLab's project", "x/y"),
        gitlab,
    )
    .unwrap();
    assert_eq!(
        (
            origin.forge.as_str(),
            origin.owner.as_str(),
            origin.repository.as_str()
        ),
        ("gitlab", "group/sub", "group/sub/cli")
    );
    assert_eq!(origin.topics, ["cli"]);
    assert_eq!(origin.visibility, Visibility::Public);
    let silent = answered_origin(
        Forge::GitHub,
        ("github.com", "GitHub's repository", "a/b"),
        br#"{"id":1}"#,
    )
    .unwrap();
    assert_eq!(silent.visibility, Visibility::Private);
    assert!(
        answered_origin(
            Forge::GitHub,
            ("github.com", "GitHub's repository", "a/b"),
            br#"{"id":1,"topics":3}"#
        )
        .is_err()
    );
    let lone = origin_of_lone();
    assert_eq!(lone.owner, "");
}

fn origin_of_lone() -> Origin {
    origin(
        Forge::GitLab,
        "gitlab.example",
        (
            "1".to_owned(),
            Delivered {
                name: "lone".to_owned(),
                path: "lone".to_owned(),
                ..Delivered::default()
            },
        ),
    )
}

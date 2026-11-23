use proptest::prelude::*;
use serde_json::json;

use super::*;

fn answer(status: u16, body: &serde_json::Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

#[test]
fn github_lists_each_tag_by_name_alone() {
    let listed = Forge::GitHub
        .tags(
            &answer(
                200,
                &json!([
                    {"name": "v2", "commit": {"sha": "b"}},
                    {"name": "v1", "commit": {"sha": "a"}}
                ]),
            ),
            1 << 20,
        )
        .unwrap();
    assert_eq!(
        listed,
        [
            Listed::Referenced("v2".to_owned()),
            Listed::Referenced("v1".to_owned())
        ]
    );
    assert_eq!(listed[0].name(), "v2");
}

#[test]
fn gitlab_lists_each_tag_in_full_with_the_annotated_date_before_the_commit_s() {
    let listed = Forge::GitLab
        .tags(
            &answer(
                200,
                &json!([
                    {
                        "name": "v2.0.0",
                        "message": "Second",
                        "created_at": "2026-11-02T10:00:00Z",
                        "commit": {"committed_date": "2026-11-01T10:00:00Z"}
                    },
                    {
                        "name": "v1.0.0",
                        "message": "  ",
                        "created_at": null,
                        "commit": {"committed_date": "2026-10-01T10:00:00Z"}
                    },
                    {"name": "bare", "message": null, "created_at": null, "commit": null}
                ]),
            ),
            1 << 20,
        )
        .unwrap();
    assert_eq!(
        listed,
        [
            Listed::Read(ForgeTag {
                name: "v2.0.0".to_owned(),
                message: Some("Second".to_owned()),
                date: Some(at("2026-11-02T10:00:00Z")),
            }),
            Listed::Read(ForgeTag {
                name: "v1.0.0".to_owned(),
                message: None,
                date: Some(at("2026-10-01T10:00:00Z")),
            }),
            Listed::Read(ForgeTag {
                name: "bare".to_owned(),
                message: None,
                date: None,
            }),
        ]
    );
}

#[test]
fn forgejo_lists_a_lightweight_tag_in_full_and_an_annotated_one_by_its_object() {
    let listed = Forge::Forgejo
        .tags(
            &answer(
                200,
                &json!([
                    {
                        "name": "v2",
                        "message": "Annotated\n",
                        "id": "tagobject",
                        "commit": {"sha": "c2", "created": "2026-11-01T10:00:00Z"}
                    },
                    {
                        "name": "v1",
                        "message": "the commit's message",
                        "id": "c1",
                        "commit": {"sha": "c1", "created": "2026-10-01T10:00:00Z"}
                    }
                ]),
            ),
            1 << 20,
        )
        .unwrap();
    assert_eq!(
        listed,
        [
            Listed::Annotated {
                name: "v2".to_owned(),
                object: "tagobject".to_owned(),
                message: Some("Annotated\n".to_owned()),
            },
            Listed::Read(ForgeTag {
                name: "v1".to_owned(),
                message: None,
                date: Some(at("2026-10-01T10:00:00Z")),
            }),
        ]
    );
    assert_eq!(listed[0].name(), "v2");
    assert_eq!(listed[1].name(), "v1");
}

#[test]
fn a_tags_answer_that_is_not_a_success_is_named_by_the_forge() {
    for forge in Forge::ALL {
        let error = forge
            .tags(&answer(500, &json!({})), 1 << 20)
            .unwrap_err()
            .to_string();
        assert!(error.contains(forge.api().tags), "{error}");
    }
}

#[test]
fn a_github_reference_points_at_a_tag_object_or_a_commit() {
    let reference = |kind: &str| answer(200, &json!({"object": {"type": kind, "sha": "s"}}));
    assert_eq!(
        Forge::GitHub.pointed(&reference("tag")),
        Ok(Pointed::Tag("s".to_owned()))
    );
    assert_eq!(
        Forge::GitHub.pointed(&reference("commit")),
        Ok(Pointed::Commit("s".to_owned()))
    );
    assert_eq!(
        Forge::GitHub.pointed(&reference("tree")).unwrap_err(),
        SourceError::Unreadable {
            what: "GitHub's tag",
            reason: "the tag points at a tree, not a commit or a tag object".to_owned(),
        }
    );
    assert!(
        Forge::GitHub
            .pointed(&answer(404, &json!({})))
            .unwrap_err()
            .to_string()
            .contains("GitHub's tag")
    );
}

#[test]
fn a_tag_object_gives_its_message_and_its_tagger_s_date() {
    let object = |message: serde_json::Value| {
        answer(
            200,
            &json!({"tag": "v1", "message": message, "tagger": {"date": "2026-11-03T10:00:00Z"}}),
        )
    };
    assert_eq!(
        Forge::Forgejo.tag_object(&object(json!("Notes")), Some("listed".to_owned())),
        Ok(ForgeTag {
            name: "v1".to_owned(),
            message: Some("Notes".to_owned()),
            date: Some(at("2026-11-03T10:00:00Z")),
        })
    );
    assert_eq!(
        Forge::Forgejo
            .tag_object(&object(json!("")), Some("listed".to_owned()))
            .map(|tag| tag.message),
        Ok(Some("listed".to_owned()))
    );
    assert_eq!(
        Forge::GitHub
            .tag_object(&answer(200, &json!({"tag": "v1"})), None)
            .map(|tag| (tag.message, tag.date)),
        Ok((None, None))
    );
    assert!(
        Forge::Forgejo
            .tag_object(&answer(404, &json!({})), None)
            .unwrap_err()
            .to_string()
            .contains("Forgejo's tag")
    );
}

#[test]
fn a_lightweight_tag_takes_its_commit_s_date() {
    assert_eq!(
        Forge::GitHub.committed(
            &answer(200, &json!({"committer": {"date": "2026-11-04T10:00:00Z"}})),
            "v1"
        ),
        Ok(ForgeTag {
            name: "v1".to_owned(),
            message: None,
            date: Some(at("2026-11-04T10:00:00Z")),
        })
    );
    assert_eq!(
        Forge::GitHub
            .committed(&answer(200, &json!({})), "v1")
            .map(|tag| tag.date),
        Ok(None)
    );
    assert!(
        Forge::GitHub
            .committed(&answer(500, &json!({})), "v1")
            .is_err()
    );
}

#[test]
fn a_tag_s_page_is_where_its_release_will_be_or_gitlab_s_tag_page() {
    assert_eq!(
        Forge::GitHub.tag_page("https://github.com/o/r/", "release/v1 b"),
        "https://github.com/o/r/releases/tag/release/v1%20b"
    );
    assert_eq!(
        Forge::Forgejo.tag_page("https://codeberg.org/o/r", "v1"),
        "https://codeberg.org/o/r/releases/tag/v1"
    );
    assert_eq!(
        Forge::GitLab.tag_page("https://gitlab.com/g/s/p", "v1"),
        "https://gitlab.com/g/s/p/-/tags/v1"
    );
}

proptest! {
    #[test]
    fn whatever_a_forge_answers_its_tag_parsers_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let response = Response { status: 200, headers: Vec::new(), body: bytes };
        for forge in Forge::ALL {
            let _ = forge.tags(&response, 1 << 20);
            let _ = forge.pointed(&response);
            let _ = forge.tag_object(&response, None);
            let _ = forge.committed(&response, "v1");
        }
    }
}

#[test]
fn a_github_tag_is_read_through_its_reference_whatever_its_answer() {
    assert_eq!(
        Forge::GitHub.tag("v1", &answer(404, &json!({}))),
        Ok(Listed::Referenced("v1".to_owned()))
    );
}

#[test]
fn every_tag_read_answered_with_a_rate_limit_says_it_is_one() {
    let limited = answer(429, &json!({}));
    let said = |error: SourceError| match error {
        SourceError::RateLimited { reason } => reason,
        other => panic!("{other:?}"),
    };
    for forge in Forge::ALL {
        let reason = said(forge.tags(&limited, 1 << 20).unwrap_err());
        assert!(reason.contains(forge.api().tags), "{reason}");
        for reason in [
            said(forge.pointed(&limited).unwrap_err()),
            said(forge.tag_object(&limited, None).unwrap_err()),
            said(forge.committed(&limited, "v1").unwrap_err()),
        ] {
            assert!(reason.contains(forge.api().tag), "{reason}");
        }
    }
    for forge in [Forge::GitLab, Forge::Forgejo] {
        let reason = said(forge.tag("v1", &limited).unwrap_err());
        assert!(reason.contains(forge.api().tag), "{reason}");
    }
}

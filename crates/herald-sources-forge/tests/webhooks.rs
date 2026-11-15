//! Each forge's webhook of a tag made, read as the source it reaches says.

#[cfg(test)]
mod webhooks {
    use herald_core::{PrereleaseRule, Selection, Signal};
    use herald_sources_forge::{Forge, Hooked, Signals};

    const OTHER_EVENT: &str = "the delivery is not of a release or a tag";
    const UNTAGGED: &str = "the source announces no tags of the repository";
    const UNSELECTED: &str = "the repository is not one the source takes";
    const DELETED: &str = "0000000000000000000000000000000000000000";

    fn tags() -> Signals {
        Signals {
            taken: [Signal::Tag].into(),
            prerelease: PrereleaseRule::Either,
        }
    }

    fn created(kind: &str) -> String {
        format!(
            r#"{{"ref":"v1","ref_type":"{kind}","repository":{{"id":1,"name":"cli","full_name":"cli/cli"}}}}"#
        )
    }

    fn pushed(after: &str) -> String {
        format!(
            r#"{{"object_kind":"tag_push","ref":"refs/tags/v1","after":"{after}","project":{{"id":1,"path_with_namespace":"cli/cli","visibility_level":20}}}}"#
        )
    }

    #[test]
    fn a_webhook_of_a_tag_made_asks_a_poll_of_a_source_announcing_tags() {
        let cases = [
            (Forge::GitHub, ("X-GitHub-Event", "create"), created("tag")),
            (Forge::Forgejo, ("X-Gitea-Event", "create"), created("tag")),
            (
                Forge::GitLab,
                ("X-Gitlab-Event", "Tag Push Hook"),
                pushed("abc"),
            ),
        ];
        for (forge, header, payload) in &cases {
            let hooked = |repository: &str, signals: &Signals| {
                forge.hooked((&[*header], payload.as_bytes()), repository, signals)
            };
            assert_eq!(hooked("CLI/cli", &tags()), Ok(Hooked::Poll), "{forge:?}");
            assert_eq!(
                hooked("cli/cli", &Signals::default()),
                Ok(Hooked::Nothing(UNTAGGED))
            );
            assert!(hooked("cli/other", &tags()).is_err());
            let owned = |selection: &Selection| {
                forge.hooked_owned(
                    (&[*header], payload.as_bytes()),
                    ("cli", false),
                    (selection, &|_| tags()),
                )
            };
            assert_eq!(owned(&Selection::default()), Ok(Hooked::Poll), "{forge:?}");
            let unselected = Selection {
                names: Some(vec!["other".to_owned()]),
                ..Selection::default()
            };
            assert_eq!(owned(&unselected), Ok(Hooked::Nothing(UNSELECTED)));
        }
    }

    #[test]
    fn a_branch_a_deleted_tag_or_another_event_is_no_tag_made() {
        let (branch, deleted, tag) = (created("branch"), pushed(DELETED), created("tag"));
        for (forge, header, payload) in [
            (Forge::GitHub, ("X-GitHub-Event", "create"), branch.as_str()),
            (
                Forge::GitLab,
                ("X-Gitlab-Event", "Tag Push Hook"),
                deleted.as_str(),
            ),
            (Forge::GitHub, ("X-GitHub-Event", "push"), tag.as_str()),
            (Forge::GitHub, ("X-GitHub-Event", "create"), "not json"),
        ] {
            assert_eq!(
                forge.hooked((&[header], payload.as_bytes()), "cli/cli", &tags()),
                Ok(Hooked::Nothing(OTHER_EVENT))
            );
            assert_eq!(
                forge.hooked_owned(
                    (&[header], payload.as_bytes()),
                    ("cli", false),
                    (&Selection::default(), &|_| tags()),
                ),
                Ok(Hooked::Nothing(OTHER_EVENT))
            );
        }
    }
}

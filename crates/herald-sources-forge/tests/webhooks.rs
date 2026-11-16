//! Each forge's webhook of a tag made or a package published, read as the
//! source it reaches says.

#[cfg(test)]
mod webhooks {
    use herald_core::{PrereleaseRule, Selection, Signal};
    use herald_sources_forge::{Forge, Hooked, Signals};

    const OTHER_EVENT: &str = "the delivery is not of a release, a tag or a package";
    const UNTAGGED: &str = "the source announces no tags of the repository";
    const UNSELECTED: &str = "the repository is not one the source takes";
    const DELETED: &str = "0000000000000000000000000000000000000000";
    const UNPACKAGED: &str = "the source announces no packages of the repository";
    const UNLINKED: &str = "the package is linked to no repository";

    fn packages() -> Signals {
        Signals {
            taken: [Signal::Package].into(),
            prerelease: PrereleaseRule::Either,
        }
    }

    fn published(action: &str, linked: bool) -> String {
        let repository = if linked {
            r#","repository":{"id":1,"name":"cli","full_name":"cli/cli"}"#
        } else {
            ""
        };
        format!(r#"{{"action":"{action}","package":{{"name":"cli"}}{repository}}}"#)
    }

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

    #[test]
    fn a_webhook_of_a_package_published_asks_a_poll_of_a_source_announcing_packages() {
        let cases = [
            (
                Forge::GitHub,
                ("X-GitHub-Event", "registry_package"),
                "published",
            ),
            (Forge::GitHub, ("X-GitHub-Event", "package"), "updated"),
            (Forge::Forgejo, ("X-Forgejo-Event", "package"), "created"),
            (Forge::Forgejo, ("X-Gitea-Event", "package"), "created"),
        ];
        for (forge, header, action) in cases {
            let payload = published(action, true);
            let hooked = |repository: &str, signals: &Signals| {
                forge.hooked((&[header], payload.as_bytes()), repository, signals)
            };
            assert_eq!(
                hooked("CLI/cli", &packages()),
                Ok(Hooked::Poll),
                "{forge:?}"
            );
            assert_eq!(
                hooked("cli/cli", &tags()),
                Ok(Hooked::Nothing(UNPACKAGED)),
                "{forge:?}"
            );
            assert!(hooked("cli/other", &packages()).is_err());
            let owned = |selection: &Selection, signals: Signals| {
                forge.hooked_owned(
                    (&[header], payload.as_bytes()),
                    ("cli", false),
                    (selection, &move |_| signals.clone()),
                )
            };
            assert_eq!(
                owned(&Selection::default(), packages()),
                Ok(Hooked::Poll),
                "{forge:?}"
            );
            assert_eq!(
                owned(&Selection::default(), tags()),
                Ok(Hooked::Nothing(UNPACKAGED))
            );
            let unselected = Selection {
                names: Some(vec!["other".to_owned()]),
                ..Selection::default()
            };
            assert_eq!(
                owned(&unselected, packages()),
                Ok(Hooked::Nothing(UNSELECTED))
            );
            let unlinked = published(action, false);
            assert_eq!(
                forge.hooked((&[header], unlinked.as_bytes()), "cli/cli", &packages()),
                Ok(Hooked::Nothing(UNLINKED))
            );
            assert_eq!(
                forge.hooked_owned(
                    (&[header], unlinked.as_bytes()),
                    ("cli", false),
                    (&Selection::default(), &|_| packages()),
                ),
                Ok(Hooked::Nothing(UNLINKED))
            );
        }
    }

    #[test]
    fn a_package_deleted_or_a_gitlab_delivery_is_no_package_published() {
        let deleted = published("deleted", true);
        let created = published("created", true);
        let published = published("published", true);
        for (forge, header, payload) in [
            (
                Forge::GitHub,
                ("X-GitHub-Event", "registry_package"),
                deleted.as_str(),
            ),
            (Forge::GitHub, ("X-GitHub-Event", "package"), "not json"),
            (
                Forge::Forgejo,
                ("X-Forgejo-Event", "package"),
                deleted.as_str(),
            ),
            (
                Forge::Forgejo,
                ("X-Forgejo-Event", "package"),
                published.as_str(),
            ),
            (
                Forge::GitLab,
                ("X-Gitlab-Event", "package"),
                created.as_str(),
            ),
        ] {
            assert_eq!(
                forge.hooked((&[header], payload.as_bytes()), "cli/cli", &packages()),
                Ok(Hooked::Nothing(OTHER_EVENT)),
                "{forge:?} {payload}"
            );
        }
    }
}

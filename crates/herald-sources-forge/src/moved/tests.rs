use super::*;

const REPOSITORY: &[u8] =
    br#"{"action":"created","repository":{"id":1,"full_name":"Lemonfiber/herald"}}"#;
const INSTALLED: &[u8] =
    br#"{"action":"added","installation":{"id":9,"account":{"login":"lemonfiber"}},"repositories_added":[]}"#;

#[test]
fn a_repository_event_names_its_repository_s_owner_and_an_installation_s_its_account() {
    assert_eq!(
        moved(
            Forge::GitHub,
            &[("X-GitHub-Event", "repository")],
            REPOSITORY
        ),
        Some(Ok("Lemonfiber".to_owned()))
    );
    assert_eq!(
        moved(
            Forge::GitHub,
            &[("x-github-event", "installation_repositories")],
            INSTALLED
        ),
        Some(Ok("lemonfiber".to_owned()))
    );
    for header in ["X-Forgejo-Event", "X-Gitea-Event"] {
        assert_eq!(
            moved(Forge::Forgejo, &[(header, "repository")], REPOSITORY),
            Some(Ok("Lemonfiber".to_owned()))
        );
    }
    assert_eq!(
        moved(
            Forge::GitHub,
            &[("X-GitHub-Event", "repository")],
            br#"{"repository":{"id":1,"full_name":"herald"}}"#
        ),
        Some(Ok(String::new()))
    );
}

#[test]
fn another_event_or_forge_says_nothing_of_repositories_changing() {
    assert_eq!(
        moved(Forge::GitHub, &[("X-GitHub-Event", "release")], REPOSITORY),
        None
    );
    assert_eq!(
        moved(
            Forge::Forgejo,
            &[("X-Forgejo-Event", "installation_repositories")],
            INSTALLED
        ),
        None
    );
    assert_eq!(
        moved(
            Forge::GitLab,
            &[("X-Gitlab-Event", "repository")],
            REPOSITORY
        ),
        None
    );
    assert_eq!(moved(Forge::GitHub, &[], REPOSITORY), None);
}

#[test]
fn a_repository_event_naming_neither_is_unreadable() {
    assert_eq!(
        moved(Forge::GitHub, &[("X-GitHub-Event", "repository")], b"{}"),
        Some(Err(SourceError::Unreadable {
            what: MOVED,
            reason: "it names no repository and no installation".to_owned(),
        }))
    );
    assert!(matches!(
        moved(Forge::GitHub, &[("X-GitHub-Event", "repository")], b"[]"),
        Some(Err(SourceError::Unreadable { what: MOVED, .. }))
    ));
}

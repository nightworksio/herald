use herald_kinds::{Fault, Hooked, SourceConfig, SourceKind};

use super::*;

fn options(repository: &str) -> SourceOptions {
    SourceOptions {
        repository: repository.to_owned(),
        token: Some(SecretRef {
            secret: "GITHUB_TOKEN".to_owned(),
        }),
        webhook: Some(SecretRef {
            secret: "HERALD_WEBHOOK".to_owned(),
        }),
        backfill: 2,
        ..SourceOptions::default()
    }
}

#[test]
fn each_forge_is_a_kind_of_source_named_as_the_config_spells_it() {
    assert_eq!(
        [GitHubKind::NAME, GitLabKind::NAME, ForgejoKind::NAME],
        ["github", "gitlab", "forgejo"]
    );
    for (checked, forge) in [
        (GitHubKind::checked(options("cli/cli")), Forge::GitHub),
        (
            GitLabKind::checked(options("group/sub/name")),
            Forge::GitLab,
        ),
        (
            ForgejoKind::checked(options("forgejo/forgejo")),
            Forge::Forgejo,
        ),
    ] {
        let source = checked.unwrap();
        assert_eq!(source.forge, forge);
        assert_eq!(source.kind(), forge.kind());
        assert_eq!(source.scheme(), Some(forge.scheme()));
    }
}

#[test]
fn a_repository_the_forge_cannot_name_keeps_the_source_out() {
    let refused = GitHubKind::checked(options("not a repository")).unwrap_err();
    assert_eq!(
        refused,
        Fault::here(
            options("not a repository")
                .check(Forge::GitHub)
                .unwrap_err()
        )
    );
}

#[test]
fn a_forge_source_names_its_secrets_and_reads_its_webhooks() {
    let source = GitHubKind::checked(options("cli/cli")).unwrap();
    assert_eq!(
        source.secret_refs(),
        vec![
            &SecretRef {
                secret: "GITHUB_TOKEN".to_owned()
            },
            &SecretRef {
                secret: "HERALD_WEBHOOK".to_owned()
            }
        ]
    );
    assert_eq!(source.kinds(), &[Kind::RELEASE]);
    assert_eq!(source.backfill(), 2);
    assert!(!source.moving_tags());
    assert!(matches!(
        source.hooked(&[("X-GitHub-Event", "ping")], b"{}"),
        Ok(Hooked::Nothing(_))
    ));
    assert!(
        source
            .hooked(&[("X-GitHub-Event", "release")], b"[")
            .is_err()
    );
    let poll = source.poller(Some(Secret::new("GITHUB_TOKEN", "t")));
    assert_eq!(poll.api(), "https://api.github.com");
    assert_eq!(poll.remembered(), herald_wire::Reading::REMEMBERED);
}

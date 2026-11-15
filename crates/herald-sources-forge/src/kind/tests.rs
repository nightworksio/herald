use herald_kinds::{Fault, Hooked, SourceConfig, SourceKind};

use super::*;

fn options(repository: &str) -> SourceOptions {
    SourceOptions {
        repository: Some(repository.to_owned()),
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
        assert_eq!(
            source.notes(),
            forge.unsigned().into_iter().collect::<Vec<_>>()
        );
    }
    assert_eq!(Forge::GitHub.unsigned(), None);
    assert_eq!(Forge::Forgejo.unsigned(), None);
    assert!(
        Forge::GitLab
            .unsigned()
            .is_some_and(|note| note.starts_with("GitLab signs a webhook only"))
    );
    let polled = GitLabKind::checked(SourceOptions {
        webhook: None,
        ..options("group/name")
    })
    .unwrap();
    assert_eq!(polled.notes(), Vec::<&str>::new());
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
    let herald_kinds::Polling::One(poll) = source.poller(Some(Secret::new("GITHUB_TOKEN", "t")))
    else {
        panic!("a source of one is polled as one");
    };
    assert_eq!(poll.api(), "https://api.github.com");
    assert_eq!(poll.remembered(), herald_wire::Reading::REMEMBERED);
}

#[test]
fn an_owner_source_is_polled_as_its_members_and_says_whether_it_takes_unlisted_ones() {
    let owned = |visibility: Option<Vec<herald_core::Visibility>>| {
        GitLabKind::checked(SourceOptions {
            owner: Some("lemonfiber".to_owned()),
            visibility,
            ..SourceOptions::default()
        })
        .unwrap()
    };
    let public = owned(None);
    assert!(!public.unlisted());
    let herald_kinds::Polling::Many(expand) = public.poller(None) else {
        panic!("an owner source is polled as its members");
    };
    assert_eq!(expand.api(), "https://gitlab.com/api/v4");
    assert!(owned(Some(vec![herald_core::Visibility::Internal])).unlisted());
    assert!(!GitHubKind::checked(options("cli/cli")).unwrap().unlisted());
}

#[test]
fn a_member_file_beside_the_config_is_read_once_and_its_names_kept() {
    let filed = |file: &str| SourceOptions {
        owner: Some("lemonfiber".to_owned()),
        members: Some(Members::File {
            file: file.to_owned(),
        }),
        tables: vec!["repo".to_owned()],
        ..SourceOptions::default()
    };
    let read = |path: &str| match path {
        "repos.toml" => Ok("[[repo]]\nname = \"herald\"\n[[repo]]\nname = \"spec\"\n".to_owned()),
        "bare.toml" => Ok("[[other]]\nname = \"x\"\n".to_owned()),
        _ => Err(format!("no file {path}")),
    };
    for kind_read in [GitHubKind::read, GitLabKind::read, ForgejoKind::read] {
        let source = kind_read(GitHubKind::checked(filed("repos.toml")).unwrap(), &read).unwrap();
        assert_eq!(
            source.names,
            Some(vec!["herald".to_owned(), "spec".to_owned()])
        );
    }
    let unread = GitHubKind::read(GitHubKind::checked(filed("gone.toml")).unwrap(), &read);
    assert_eq!(unread, Err(Fault::at("members.file", "no file gone.toml")));
    let Err(fault) = GitHubKind::read(GitHubKind::checked(filed("bare.toml")).unwrap(), &read)
    else {
        panic!("a file without the tables named keeps the source out");
    };
    assert_eq!(fault.key.as_deref(), Some("members.file"));
    assert!(
        fault.message.contains("repo is not an array"),
        "{}",
        fault.message
    );
    let unfiled = GitHubKind::checked(SourceOptions {
        owner: Some("lemonfiber".to_owned()),
        ..SourceOptions::default()
    })
    .unwrap();
    assert_eq!(GitHubKind::read(unfiled.clone(), &read), Ok(unfiled));
}

#[test]
fn an_owner_source_takes_webhooks_of_the_names_its_member_file_lists_and_says_to_poll_too() {
    const EVENT: &str = include_str!("../../fixtures/github/release-event.reference.json");
    let hooking = |names: Option<Vec<String>>| ForgeSource {
        names,
        ..GitHubKind::checked(SourceOptions {
            owner: Some("cli".to_owned()),
            webhook: Some(SecretRef {
                secret: "HERALD_WEBHOOK".to_owned(),
            }),
            ..SourceOptions::default()
        })
        .unwrap()
    };
    let headers = [("X-GitHub-Event", "release")];
    assert!(matches!(
        hooking(None).hooked(&headers, EVENT.as_bytes()),
        Ok(Hooked::Member { .. })
    ));
    assert_eq!(
        hooking(Some(vec!["other".to_owned()])).hooked(&headers, EVENT.as_bytes()),
        Ok(Hooked::Nothing(
            "the repository is not one the source takes"
        ))
    );
    assert_eq!(hooking(None).notes(), [OWNER_HOOKED]);
    let grouped = GitLabKind::checked(SourceOptions {
        owner: Some("cli".to_owned()),
        webhook: Some(SecretRef {
            secret: "HERALD_WEBHOOK".to_owned(),
        }),
        ..SourceOptions::default()
    })
    .unwrap();
    assert_eq!(
        grouped.notes(),
        [Forge::GitLab.unsigned().unwrap(), OWNER_HOOKED]
    );
}

#[test]
fn a_source_of_one_repository_names_its_project_and_an_owner_none() {
    assert_eq!(
        GitLabKind::checked(options("group/sub/cli"))
            .unwrap()
            .projects(),
        ["cli"]
    );
    assert_eq!(
        GitHubKind::checked(SourceOptions {
            owner: Some("cli".to_owned()),
            ..SourceOptions::default()
        })
        .unwrap()
        .projects(),
        Vec::<String>::new()
    );
}

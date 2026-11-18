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
        assert!(source.origins());
        assert_eq!(source.routed(), Vec::new());
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
    let kinds = |signals: &[herald_core::Signal]| {
        GitHubKind::checked(SourceOptions {
            signals: Some(signals.to_vec()),
            ..options("cli/cli")
        })
        .unwrap()
        .kinds()
    };
    assert_eq!(
        kinds(&[herald_core::Signal::Tag, herald_core::Signal::Package]),
        &[Kind::RELEASE, Kind::PACKAGE_VERSION]
    );
    assert_eq!(
        kinds(&[herald_core::Signal::Release, herald_core::Signal::Package]),
        &[Kind::RELEASE, Kind::PACKAGE_VERSION]
    );
    assert_eq!(
        kinds(&[herald_core::Signal::Package]),
        &[Kind::PACKAGE_VERSION]
    );
    assert_eq!(kinds(&[herald_core::Signal::Tag]), &[Kind::RELEASE]);
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
    let herald_kinds::Polling::One(poll) = source.poller(
        Some(herald_kinds::Credential::Named(herald_ports::Secret::new(
            "GITHUB_TOKEN",
            "t",
        ))),
        false,
    ) else {
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
    let herald_kinds::Polling::Many(expand) = public.poller(None, false) else {
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
        [
            Forge::GitLab.unsigned().unwrap(),
            OWNER_HOOKED,
            super::GITLAB_GROUPED
        ]
    );
    let hooked = GitLabKind::checked(SourceOptions {
        owner: Some("cli".to_owned()),
        webhook: Some(SecretRef {
            secret: "HERALD_WEBHOOK".to_owned(),
        }),
        member_hooks: Some(crate::MemberHooks::Setup),
        ..SourceOptions::default()
    })
    .unwrap();
    assert_eq!(
        hooked.notes(),
        [Forge::GitLab.unsigned().unwrap(), OWNER_HOOKED]
    );
    assert_eq!(hooked.registering(), herald_kinds::Registering::Setup);
}

#[test]
fn a_github_source_naming_an_app_reads_as_its_installation() {
    let installed = GitHubKind::checked(SourceOptions {
        app: Some("herald".to_owned()),
        token: None,
        ..options("cli/cli")
    })
    .unwrap();
    assert_eq!(
        installed
            .installed()
            .map(|installed| installed.installation),
        Some(herald_kinds::Installation::Repository("cli/cli"))
    );
    assert_eq!(
        GitHubKind::checked(options("cli/cli")).unwrap().installed(),
        None
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
    assert_eq!(
        GitLabKind::checked(options("group/sub/cli"))
            .unwrap()
            .standing(),
        Some(herald_kinds::Standing::new(
            "https://gitlab.com/api/v4",
            "group/sub/cli",
            false
        ))
    );
    assert_eq!(
        GitHubKind::checked(SourceOptions {
            owner: Some("cli".to_owned()),
            ..SourceOptions::default()
        })
        .unwrap()
        .standing(),
        None
    );
}

#[test]
fn a_source_holds_a_tag_where_it_prefers_releases_and_takes_both() {
    let holding = |signals: Option<Vec<herald_core::Signal>>, prefer| {
        GitHubKind::checked(SourceOptions {
            signals,
            prefer,
            ..options("cli/cli")
        })
        .unwrap()
        .holds()
    };
    let both = || Some(vec![herald_core::Signal::Release, herald_core::Signal::Tag]);
    assert!(holding(both(), None));
    assert!(!holding(both(), Some(herald_core::Prefer::First)));
    assert!(!holding(Some(vec![herald_core::Signal::Release]), None));
    assert!(!holding(Some(vec![herald_core::Signal::Tag]), None));
    let signalled = |signals| {
        GitHubKind::checked(SourceOptions {
            signals,
            ..options("cli/cli")
        })
        .unwrap()
        .signals()
    };
    assert_eq!(signalled(None), [herald_core::Signal::Release]);
    assert_eq!(
        signalled(both()),
        [herald_core::Signal::Release, herald_core::Signal::Tag]
    );
    assert_eq!(
        signalled(Some(vec![herald_core::Signal::Tag])),
        [herald_core::Signal::Tag]
    );
}

#[test]
fn each_poll_of_a_source_holding_tags_says_how_long_a_tag_waits() {
    let both = || Some(vec![herald_core::Signal::Release, herald_core::Signal::Tag]);
    let one = GitHubKind::checked(SourceOptions {
        signals: both(),
        ..options("cli/cli")
    })
    .unwrap();
    let Polling::One(poll) = one.poller(None, false) else {
        panic!("a repository is polled as one");
    };
    assert_eq!(poll.tag_window(), Some(crate::TAG_WINDOW));
    assert_eq!(one.tag_window(), Some(crate::TAG_WINDOW));
    let plain = GitHubKind::checked(options("cli/cli")).unwrap();
    let Polling::One(poll) = plain.poller(None, false) else {
        panic!("a repository is polled as one");
    };
    assert_eq!(poll.tag_window(), None);
    let owner = GitHubKind::checked(SourceOptions {
        repository: None,
        owner: Some("cli".to_owned()),
        signals: both(),
        repositories: std::collections::BTreeMap::from([(
            "quiet".to_owned(),
            crate::Override {
                signals: Some(vec![herald_core::Signal::Release]),
                ..crate::Override::default()
            },
        )]),
        ..options("cli/cli")
    })
    .unwrap();
    assert_eq!(owner.tag_window(), None);
    assert_eq!(
        GitHubKind::checked(options("cli/cli"))
            .unwrap()
            .tag_window(),
        None
    );
    let Polling::Many(expand) = owner.poller(None, false) else {
        panic!("an owner is polled as its members");
    };
    let member = |name: &str| herald_core::Listed {
        id: "1".to_owned(),
        name: name.to_owned(),
        path: format!("cli/{name}"),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    assert_eq!(expand.tag_window(&member("cli")), Some(crate::TAG_WINDOW));
    assert_eq!(expand.tag_window(&member("quiet")), None);
    assert_eq!(
        expand.member(&member("cli")).tag_window(),
        Some(crate::TAG_WINDOW)
    );
}

#[test]
fn an_owner_source_names_each_route_its_overrides_name() {
    let owner = GitHubKind::checked(SourceOptions {
        repository: None,
        owner: Some("cli".to_owned()),
        repositories: std::collections::BTreeMap::from([(
            "plugin-*".to_owned(),
            crate::Override {
                routes: Some(vec!["plugins".to_owned()]),
                ..crate::Override::default()
            },
        )]),
        ..options("cli/cli")
    })
    .unwrap();
    assert_eq!(
        owner.routed(),
        [(
            "repositories.\"plugin-*\".routes".to_owned(),
            "plugins".to_owned()
        )]
    );
    assert!(owner.names_route(&herald_core::RouteId::new("plugins")));
}

#[test]
fn an_owner_discovering_by_push_says_so_and_what_it_cannot_hear() {
    let owner = |forge: fn(SourceOptions) -> Result<ForgeSource, Fault>,
                 discovery: Option<crate::Discovery>,
                 webhook: bool| {
        forge(SourceOptions {
            repository: None,
            owner: Some("cli".to_owned()),
            discovery,
            webhook: webhook.then(|| SecretRef {
                secret: "HERALD_WEBHOOK".to_owned(),
            }),
            ..options("cli/cli")
        })
        .unwrap()
    };
    let listing = owner(GitHubKind::checked, None, true);
    assert!(!listing.pushed());
    let Polling::Many(expand) = listing.poller(None, false) else {
        panic!("an owner is polled as its members");
    };
    assert!(!expand.pushed());
    let pushed = owner(GitHubKind::checked, Some(crate::Discovery::Push), true);
    assert!(pushed.pushed());
    let Polling::Many(expand) = pushed.poller(None, false) else {
        panic!("an owner is polled as its members");
    };
    assert!(expand.pushed());
    assert_eq!(pushed.notes(), [super::OWNER_HOOKED]);
    assert_eq!(
        owner(GitHubKind::checked, Some(crate::Discovery::Push), false).notes(),
        [super::UNPUSHED]
    );
    assert_eq!(
        owner(GitLabKind::checked, Some(crate::Discovery::Push), false).notes(),
        [super::UNPUSHED, super::GITLAB_UNPUSHED]
    );
    assert_eq!(
        owner(GitLabKind::checked, Some(crate::Discovery::Push), true).notes(),
        [
            Forge::GitLab.unsigned().unwrap(),
            super::OWNER_HOOKED,
            super::GITLAB_UNPUSHED,
            super::GITLAB_GROUPED
        ]
    );
    assert_eq!(
        owner(GitHubKind::checked, None, false).notes(),
        Vec::<&str>::new()
    );
}

#[test]
fn a_minted_token_authorizes_a_request_as_its_last_refresh_left_it() {
    use std::sync::Arc;

    use herald_kinds::{Credential, Minted};

    use herald_ports::{Method, Request, Secret, SecretText};

    use crate::Token;

    let minted = Arc::new(Minted::default());
    let token = Token::of(Credential::Minted(Arc::clone(&minted)));
    let authorization = || {
        let request = Request::new(Method::Get, SecretText::plain("https://api.github.com/x"));
        crate::forge::authorized(Forge::GitHub, request, Some(&token))
            .headers
            .iter()
            .find(|(name, _)| name == "Authorization")
            .map(|(_, value)| value.expose().clone())
    };
    assert_eq!(authorization(), None);
    minted.refresh(Some(Secret::new("T", "ghs_now")));
    assert_eq!(authorization(), Some("Bearer ghs_now".to_owned()));
    assert_eq!(
        Token::of(Credential::Named(Secret::new("N", "n"))),
        Token::Named(Secret::new("N", "n"))
    );
}

#[test]
fn a_gitlab_owner_says_who_registers_its_members_webhooks() {
    for (member_hooks, registering) in [
        (None, herald_kinds::Registering::Never),
        (
            Some(crate::MemberHooks::Off),
            herald_kinds::Registering::Never,
        ),
        (
            Some(crate::MemberHooks::Setup),
            herald_kinds::Registering::Setup,
        ),
        (
            Some(crate::MemberHooks::Round),
            herald_kinds::Registering::Round,
        ),
    ] {
        let source = GitLabKind::checked(SourceOptions {
            repository: None,
            owner: Some("group".to_owned()),
            member_hooks,
            ..options("cli/cli")
        })
        .unwrap();
        let Polling::Many(expand) = source.poller(None, false) else {
            panic!("an owner is polled as its members");
        };
        assert_eq!(expand.registering(), registering);
    }
}

#[test]
fn a_gitlab_owner_asks_its_members_hooks_and_its_token_s_scopes_at_its_api() {
    use herald_ports::{Response, Secret};

    let source = GitLabKind::checked(SourceOptions {
        repository: None,
        owner: Some("group".to_owned()),
        ..options("cli/cli")
    })
    .unwrap();
    let Polling::Many(expand) = source.poller(None, false) else {
        panic!("an owner is polled as its members");
    };
    let answer = |body: &str| Response {
        status: 200,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    };
    let member = herald_core::Listed {
        id: "7".to_owned(),
        name: "a".to_owned(),
        path: "group/a".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    let url = "https://herald.example/hooks/group";
    let scripted = herald_testing::ScriptedHttp::new(vec![
        answer(&format!(r#"[{{"url":"{url}"}}]"#)),
        answer(r#"{"scopes":["api"]}"#),
        answer("[]"),
    ]);
    assert_eq!(
        pollster::block_on(expand.hooked(&scripted, &member, url)),
        Ok(true)
    );
    assert_eq!(pollster::block_on(expand.may_register(&scripted)), Ok(()));
    assert!(
        pollster::block_on(expand.register(&scripted, &member, (url, &Secret::new("HOOK", "s"))))
            .is_err()
    );
    let asked: Vec<String> = scripted.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked[..2],
        [
            "https://gitlab.com/api/v4/projects/7/hooks?per_page=100".to_owned(),
            "https://gitlab.com/api/v4/personal_access_tokens/self".to_owned(),
        ]
    );
}

mod alerts;
mod pulls;
mod runs;

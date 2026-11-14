use super::*;

fn owner(owner: &str) -> SourceOptions {
    SourceOptions {
        owner: Some(owner.to_owned()),
        ..SourceOptions::default()
    }
}

#[test]
fn a_source_names_a_repository_or_an_owner_and_not_both() {
    assert_eq!(
        SourceOptions::default().check(Forge::GitHub),
        Err("a source names a repository, as owner/name, or an owner".to_owned())
    );
    assert_eq!(
        SourceOptions {
            repository: Some("a/b".to_owned()),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Err("a source names either a repository or an owner; remove one".to_owned())
    );
    assert_eq!(owner("lemonfiber").check(Forge::GitHub), Ok(()));
    assert_eq!(owner("group/sub").check(Forge::GitLab), Ok(()));
    let deepest = vec!["g"; 21].join("/");
    assert_eq!(owner(&deepest).check(Forge::GitLab), Ok(()));
    assert!(owner(&format!("{deepest}/g")).check(Forge::GitLab).is_err());
    assert_eq!(
        owner("group/sub").check(Forge::Forgejo),
        Err("\"group/sub\" is not an owner; give it as an organization's or a user's name, of letters, digits, '.', '_' and '-'".to_owned())
    );
    assert_eq!(
        owner("..").check(Forge::GitLab),
        Err("\"..\" is not an owner; give it as a group's full path, such as group/subgroup, or a user's name, of letters, digits, '.', '_' and '-'".to_owned())
    );
    assert_eq!(
        SourceOptions {
            api: Some("ftp://x".to_owned()),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Err("\"ftp://x\" is not a web address; give the API's base URL, such as https://forge.example/api/v1".to_owned())
    );
}

fn owned_keys() -> Vec<(&'static str, SourceOptions)> {
    vec![
        (
            "include",
            SourceOptions {
                include: vec!["a".to_owned()],
                ..SourceOptions::default()
            },
        ),
        (
            "exclude",
            SourceOptions {
                exclude: vec!["a".to_owned()],
                ..SourceOptions::default()
            },
        ),
        (
            "topics",
            SourceOptions {
                topics: vec!["a".to_owned()],
                ..SourceOptions::default()
            },
        ),
        (
            "visibility",
            SourceOptions {
                visibility: Some(Vec::new()),
                ..SourceOptions::default()
            },
        ),
        (
            "archived",
            SourceOptions {
                archived: true,
                ..SourceOptions::default()
            },
        ),
        (
            "fork",
            SourceOptions {
                fork: true,
                ..SourceOptions::default()
            },
        ),
        (
            "template",
            SourceOptions {
                template: true,
                ..SourceOptions::default()
            },
        ),
        (
            "language",
            SourceOptions {
                language: vec!["Rust".to_owned()],
                ..SourceOptions::default()
            },
        ),
        (
            "properties",
            SourceOptions {
                properties: BTreeMap::from([("team".to_owned(), Values::One("a".to_owned()))]),
                ..SourceOptions::default()
            },
        ),
        (
            "subgroups",
            SourceOptions {
                subgroups: Some(true),
                ..SourceOptions::default()
            },
        ),
        (
            "most_listed",
            SourceOptions {
                most_listed: NonZeroUsize::new(1),
                ..SourceOptions::default()
            },
        ),
        (
            "most_members",
            SourceOptions {
                most_members: NonZeroUsize::new(1),
                ..SourceOptions::default()
            },
        ),
        (
            "keep_members",
            SourceOptions {
                keep_members: Some(Duration(SignedDuration::from_hours(1))),
                ..SourceOptions::default()
            },
        ),
        (
            "listed_page",
            SourceOptions {
                listed_page: NonZeroUsize::new(1),
                ..SourceOptions::default()
            },
        ),
    ]
}

#[test]
fn an_owner_s_keys_are_refused_on_a_source_of_one_repository() {
    let one = |options: SourceOptions| {
        SourceOptions {
            repository: Some("a/b".to_owned()),
            ..options
        }
        .check(Forge::GitLab)
    };
    let refused = |key: &str| {
        Err(format!(
            "{key} chooses among an owner's repositories; give owner in place of repository, or remove {key}"
        ))
    };
    let each = owned_keys();
    for (key, options) in each {
        assert_eq!(one(options), refused(key), "{key}");
    }
    assert_eq!(one(SourceOptions::default()), Ok(()));
}

#[test]
fn an_owner_source_takes_no_webhook_its_forge_s_keys_only_and_its_caps_within_bounds() {
    assert_eq!(
        SourceOptions {
            webhook: Some(SecretRef {
                secret: "HOOK".to_owned()
            }),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Err("an owner source is polled, and takes no webhook; remove webhook".to_owned())
    );
    let grouped = SourceOptions {
        subgroups: Some(false),
        ..owner("a")
    };
    assert_eq!(grouped.check(Forge::GitLab), Ok(()));
    assert_eq!(
        grouped.check(Forge::GitHub),
        Err("subgroups is a GitLab group's; remove it".to_owned())
    );
    let most = |most| SourceOptions {
        most_members: NonZeroUsize::new(most),
        ..owner("a")
    };
    assert_eq!(most(MOST_MEMBERS_ALLOWED).check(Forge::GitHub), Ok(()));
    assert_eq!(
        most(MOST_MEMBERS_ALLOWED + 1).check(Forge::GitHub),
        Err("most_members is at most 100000; lower it".to_owned())
    );
    assert_eq!(
        SourceOptions {
            exclude: vec!["[".to_owned()],
            ..owner("a")
        }
        .check(Forge::GitHub),
        Err("\"[\" opens a [ it never closes with a character in it; close it, or write [[] for a [ itself".to_owned())
    );
    assert!(
        SourceOptions {
            include: vec!["[".to_owned()],
            ..owner("a")
        }
        .selection()
        .is_err()
    );
}

#[test]
fn an_owner_source_s_defaults_take_public_repositories_within_the_shipped_caps() {
    let options = owner("a");
    assert_eq!(options.selection(), Ok(Selection::default()));
    assert_eq!(options.most_listed().get(), 10_000);
    assert_eq!(options.most_members().get(), 1_000);
    assert_eq!(options.keep_members(), SignedDuration::from_hours(720));
    assert!(options.subgroups());
    let given = SourceOptions {
        include: vec!["Plugin-*".to_owned()],
        exclude: vec!["plugin-old".to_owned()],
        topics: vec!["media".to_owned()],
        visibility: Some(vec![Visibility::Private]),
        archived: true,
        fork: true,
        template: true,
        language: vec!["Rust".to_owned()],
        properties: BTreeMap::from([
            ("team".to_owned(), Values::One("media".to_owned())),
            (
                "tier".to_owned(),
                Values::Any(vec!["core".to_owned(), "edge".to_owned()]),
            ),
        ]),
        subgroups: Some(false),
        most_listed: NonZeroUsize::new(5),
        most_members: NonZeroUsize::new(4),
        keep_members: Some(Duration(SignedDuration::from_hours(2))),
        ..owner("a")
    };
    assert_eq!(
        given.selection(),
        Ok(Selection {
            include: vec![Glob::new("Plugin-*").unwrap()],
            exclude: vec![Glob::new("plugin-old").unwrap()],
            topics: vec!["media".to_owned()],
            visibility: vec![Visibility::Private],
            archived: true,
            fork: true,
            template: true,
            language: vec!["Rust".to_owned()],
            properties: BTreeMap::from([
                ("team".to_owned(), vec!["media".to_owned()]),
                (
                    "tier".to_owned(),
                    vec!["core".to_owned(), "edge".to_owned()]
                ),
            ]),
        })
    );
    assert_eq!(
        (
            given.most_listed().get(),
            given.most_members().get(),
            given.keep_members(),
            given.subgroups()
        ),
        (5, 4, SignedDuration::from_hours(2), false)
    );
}

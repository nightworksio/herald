use std::collections::BTreeSet;

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

/// A change giving one of an owner's keys.
type Change = fn(&mut SourceOptions);

fn owned_keys() -> Vec<(&'static str, SourceOptions)> {
    let changes: [(&str, Change); 22] = [
        ("include", |options| {
            options.include = vec!["a".to_owned()];
        }),
        ("exclude", |options| {
            options.exclude = vec!["a".to_owned()];
        }),
        ("topics", |options| {
            options.topics = vec!["a".to_owned()];
        }),
        ("visibility", |options| {
            options.visibility = Some(Vec::new());
        }),
        ("archived", |options| {
            options.archived = true;
        }),
        ("fork", |options| {
            options.fork = true;
        }),
        ("template", |options| {
            options.template = true;
        }),
        ("language", |options| {
            options.language = vec!["Rust".to_owned()];
        }),
        ("properties", |options| {
            options.properties = BTreeMap::from([("a".to_owned(), Values::One("b".to_owned()))]);
        }),
        ("subgroups", |options| {
            options.subgroups = Some(true);
        }),
        ("most_listed", |options| {
            options.most_listed = NonZeroUsize::new(1);
        }),
        ("most_members", |options| {
            options.most_members = NonZeroUsize::new(1);
        }),
        ("keep_members", |options| {
            options.keep_members = Some(Duration(SignedDuration::from_hours(1)));
        }),
        ("listed_page", |options| {
            options.listed_page = NonZeroUsize::new(1);
        }),
        ("members", |options| {
            options.members = Some(Members::File {
                file: "repos.toml".to_owned(),
            });
        }),
        ("tables", |options| {
            options.tables = vec!["repo".to_owned()];
        }),
        ("key", |options| {
            options.key = Some("name".to_owned());
        }),
        ("project", |options| {
            options.project = Some("{{ repository.path }}".to_owned());
        }),
        ("repositories", |options| {
            options.repositories = BTreeMap::from([("a".to_owned(), Override::default())]);
        }),
        ("transport", |options| {
            options.transport = Some(Transport::Rest);
        }),
        ("graphql", |options| {
            options.graphql = Some("https://x.example/graphql".to_owned());
        }),
        ("graphql_batch", |options| {
            options.graphql_batch = NonZeroUsize::new(5);
        }),
    ];
    changes
        .into_iter()
        .map(|(key, change)| {
            let mut options = SourceOptions::default();
            change(&mut options);
            (key, options)
        })
        .collect()
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
fn an_owner_source_takes_webhooks_its_forge_s_keys_only_and_its_caps_within_bounds() {
    assert_eq!(
        SourceOptions {
            webhook: Some(SecretRef {
                secret: "HOOK".to_owned()
            }),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Ok(())
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
            names: None,
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

#[test]
fn a_project_template_is_checked_for_what_it_reads_when_the_config_loads() {
    let projected = |project: &str| {
        SourceOptions {
            project: Some(project.to_owned()),
            ..owner("a")
        }
        .check(Forge::GitHub)
    };
    assert_eq!(projected("{{ repository.path }}"), Ok(()));
    assert_eq!(
        projected("{{ repository.title }}"),
        Err("project reads repository.title, which a repository does not carry; use repository.id, repository.name, repository.path, repository.owner, repository.topics, repository.properties, repository.visibility".to_owned())
    );
    let Err(unclosed) = projected("{{ repository.name") else {
        panic!("a template that does not compile is refused");
    };
    assert!(unclosed.starts_with("project syntax error"), "{unclosed}");
    assert!(
        SourceOptions {
            project: Some("{{ repository.name }}".to_owned()),
            ..owner("a")
        }
        .project()
        .is_some()
    );
    assert!(owner("a").project().is_none());
}

#[test]
fn a_member_file_is_named_by_a_repository_and_a_path_inside_it() {
    let filed = |repository: &str, path: &str| {
        SourceOptions {
            members: Some(Members::Repository {
                repository: repository.to_owned(),
                path: path.to_owned(),
                reference: None,
            }),
            ..owner("a")
        }
        .check(Forge::GitHub)
    };
    assert_eq!(filed("lemonfiber/spec", "30-repos/repos.toml"), Ok(()));
    assert_eq!(
        filed("spec", "repos.toml"),
        Err("members.repository \"spec\" is not a repository; give it as owner/name".to_owned())
    );
    for path in ["../secrets.toml", "a//b.toml", "a/./b.toml", ""] {
        assert_eq!(
            filed("lemonfiber/spec", path),
            Err(format!(
                "members.path \"{path}\" is not a file's path in the repository; give it as dir/file.toml, without empty, . or .. parts"
            )),
            "{path}"
        );
    }
    for loose in [
        SourceOptions {
            tables: vec!["repo".to_owned()],
            ..owner("a")
        },
        SourceOptions {
            key: Some("name".to_owned()),
            ..owner("a")
        },
    ] {
        assert_eq!(
            loose.check(Forge::GitHub),
            Err("tables and key read a member file; give members, or remove them".to_owned())
        );
    }
    assert_eq!(owner("a").key(), "name");
    assert_eq!(
        SourceOptions {
            key: Some("repo".to_owned()),
            ..owner("a")
        }
        .key(),
        "repo"
    );
}

#[test]
fn a_member_file_table_is_one_shape_or_the_other() {
    let read = |text: &str| toml::from_str::<SourceOptions>(text).map(|options| options.members);
    assert_eq!(
        read("members = { file = \"repos.toml\" }"),
        Ok(Some(Members::File {
            file: "repos.toml".to_owned()
        }))
    );
    assert_eq!(
        read("members = { repository = \"a/spec\", path = \"r.toml\", ref = \"main\" }"),
        Ok(Some(Members::Repository {
            repository: "a/spec".to_owned(),
            path: "r.toml".to_owned(),
            reference: Some("main".to_owned()),
        }))
    );
    assert!(read("members = { file = \"repos.toml\", path = \"r.toml\" }").is_err());
}

#[test]
fn a_github_owner_names_its_transport_its_graphql_endpoint_and_its_batch() {
    let github = SourceOptions {
        transport: Some(Transport::Events),
        api: Some("https://ghe.example/api/v3".to_owned()),
        graphql: Some("https://ghe.example/api/graphql".to_owned()),
        graphql_batch: NonZeroUsize::new(GRAPHQL_BATCH_ALLOWED),
        ..owner("a")
    };
    assert_eq!(github.check(Forge::GitHub), Ok(()));
    assert_eq!(github.transport(), Transport::Events);
    assert_eq!(github.graphql_batch().get(), GRAPHQL_BATCH_ALLOWED);
    assert_eq!(owner("a").transport(), Transport::Auto);
    assert_eq!(owner("a").graphql_batch(), GRAPHQL_BATCH);
    for (key, options) in [
        (
            "transport",
            SourceOptions {
                transport: Some(Transport::Rest),
                ..owner("a")
            },
        ),
        (
            "graphql",
            SourceOptions {
                graphql: Some("https://x.example/graphql".to_owned()),
                ..owner("a")
            },
        ),
        (
            "graphql_batch",
            SourceOptions {
                graphql_batch: NonZeroUsize::new(5),
                ..owner("a")
            },
        ),
    ] {
        for forge in [Forge::GitLab, Forge::Forgejo] {
            assert_eq!(
                options.check(forge),
                Err(format!("{key} is a GitHub owner's; remove it"))
            );
        }
    }
    assert_eq!(
        SourceOptions {
            graphql: Some("ftp://x".to_owned()),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Err("graphql \"ftp://x\" is not a web address; give the GraphQL endpoint, such as https://ghe.example/api/graphql".to_owned())
    );
    assert_eq!(
        SourceOptions {
            graphql: Some("https://elsewhere.example/graphql".to_owned()),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Err("graphql \"https://elsewhere.example/graphql\" is not on api's host, the only one the source may reach; give the endpoint beside api, or remove graphql".to_owned())
    );
    assert_eq!(
        SourceOptions {
            graphql: Some("https://api.github.com/graphql".to_owned()),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Ok(())
    );
    assert_eq!(
        SourceOptions {
            graphql_batch: NonZeroUsize::new(GRAPHQL_BATCH_ALLOWED + 1),
            ..owner("a")
        }
        .check(Forge::GitHub),
        Err(format!(
            "graphql_batch is at most {GRAPHQL_BATCH_ALLOWED}; lower it"
        ))
    );
}

#[test]
fn each_override_names_its_repositories_by_a_glob_and_its_project_by_a_template() {
    let overriding = |glob: &str, project: &str| SourceOptions {
        repositories: BTreeMap::from([(
            glob.to_owned(),
            Override {
                project: Some(project.to_owned()),
                ..Override::default()
            },
        )]),
        ..owner("a")
    };
    assert_eq!(
        overriding("plugin-*", "{{ repository.name }}").check(Forge::GitHub),
        Ok(())
    );
    let Err(unclosed) = overriding("plugin-[", "x").check(Forge::GitHub) else {
        panic!("a glob that cannot be read is refused");
    };
    assert!(
        unclosed.starts_with("repositories: \"plugin-[\" opens a ["),
        "{unclosed}"
    );
    assert_eq!(
        overriding("plugin-*", "{{ event.project }}").check(Forge::GitHub),
        Err("repositories.\"plugin-*\".project reads event.project, which a repository does not carry; use repository.id, repository.name, repository.path, repository.owner, repository.topics, repository.properties, repository.visibility".to_owned())
    );
    let ordered = SourceOptions {
        repositories: BTreeMap::from([
            (
                "*".to_owned(),
                Override {
                    project: Some("all".to_owned()),
                    ..Override::default()
                },
            ),
            (
                "plugin-sonarr".to_owned(),
                Override {
                    project: Some("sonarr".to_owned()),
                    ..Override::default()
                },
            ),
            (
                "plugin-*".to_owned(),
                Override {
                    project: None,
                    ..Override::default()
                },
            ),
            (
                "plugin-?*".to_owned(),
                Override {
                    project: Some("plugin".to_owned()),
                    ..Override::default()
                },
            ),
        ]),
        ..owner("a")
    };
    assert_eq!(
        ordered
            .projects()
            .iter()
            .map(|(glob, _)| glob.as_str())
            .collect::<Vec<_>>(),
        ["plugin-sonarr", "plugin-?*", "*"]
    );
}

#[test]
fn graphql_is_chosen_by_transport_and_under_auto_by_a_token() {
    let token = Some(SecretRef {
        secret: "TOKEN".to_owned(),
    });
    let with = |transport: Option<Transport>, token: Option<SecretRef>| SourceOptions {
        token,
        transport,
        ..owner("a")
    };
    assert!(with(None, token.clone()).batched());
    assert!(!with(None, None).batched());
    assert!(with(Some(Transport::Graphql), token.clone()).batched());
    assert!(!with(Some(Transport::Rest), token.clone()).batched());
    assert!(with(Some(Transport::Events), token.clone()).batched());
    assert!(!with(Some(Transport::Events), None).batched());
    assert_eq!(
        with(Some(Transport::Graphql), None).check(Forge::GitHub),
        Err("transport graphql needs a token, as GitHub's GraphQL API answers none without; give token, or choose rest".to_owned())
    );
}

#[test]
fn two_overrides_as_narrow_as_each_other_that_could_name_one_repository_do_not_load() {
    let both = |one: &str, other: &str, project: Option<&str>| {
        SourceOptions {
            repositories: BTreeMap::from([
                (
                    one.to_owned(),
                    Override {
                        project: Some("a".to_owned()),
                        ..Override::default()
                    },
                ),
                (
                    other.to_owned(),
                    Override {
                        project: project.map(ToOwned::to_owned),
                        ..Override::default()
                    },
                ),
            ]),
            ..owner("a")
        }
        .check(Forge::GitHub)
    };
    assert_eq!(
        both("plugin-*", "*-sonarr", Some("b")),
        Err("repositories \"*-sonarr\" and \"plugin-*\" both set project, are as narrow as each other and could both name one repository; make one narrower".to_owned())
    );
    assert_eq!(both("plugin-*", "*-sonarr", None), Ok(()));
    assert_eq!(both("plugin-*", "web-*", Some("b")), Ok(()));
    assert_eq!(both("plugin-*", "plugin-s*", Some("b")), Ok(()));
}

fn signalled(signals: &[Signal], prerelease: Option<PrereleaseRule>) -> Override {
    Override {
        signals: Some(signals.to_vec()),
        prerelease,
        ..Override::default()
    }
}

#[test]
fn a_source_announces_releases_told_by_either_rule_where_it_says_nothing() {
    let source = owner("a");
    assert_eq!(source.signals_of("any"), Signals::default());
    assert_eq!(
        Signals::default(),
        Signals {
            taken: BTreeSet::from([Signal::Release]),
            prerelease: PrereleaseRule::Either,
        }
    );
    assert!(Signals::default().takes(Signal::Release));
    assert!(!Signals::default().takes(Signal::Tag));
    assert!(source.may_take(Signal::Release));
    assert!(!source.may_take(Signal::Tag));
}

#[test]
fn the_narrowest_override_that_sets_each_key_gives_it_and_the_source_the_rest() {
    let source = SourceOptions {
        signals: Some(vec![Signal::Tag]),
        prerelease: Some(PrereleaseRule::Tag),
        repositories: BTreeMap::from([
            (
                "plugin-*".to_owned(),
                signalled(&[Signal::Release], Some(PrereleaseRule::Flag)),
            ),
            (
                "plugin-sonarr".to_owned(),
                signalled(&[Signal::Release, Signal::Tag], None),
            ),
            (
                "web-*".to_owned(),
                Override {
                    project: Some("web".to_owned()),
                    ..Override::default()
                },
            ),
        ]),
        ..owner("a")
    };
    assert_eq!(source.check(Forge::GitHub), Ok(()));
    assert_eq!(
        source.signals_of("plugin-sonarr"),
        Signals {
            taken: BTreeSet::from([Signal::Release, Signal::Tag]),
            prerelease: PrereleaseRule::Flag,
        }
    );
    assert_eq!(
        source.signals_of("PLUGIN-radarr"),
        Signals {
            taken: BTreeSet::from([Signal::Release]),
            prerelease: PrereleaseRule::Flag,
        }
    );
    for name in ["web-site", "core"] {
        assert_eq!(
            source.signals_of(name),
            Signals {
                taken: BTreeSet::from([Signal::Tag]),
                prerelease: PrereleaseRule::Tag,
            }
        );
    }
    assert!(source.may_take(Signal::Tag));
    assert!(source.may_take(Signal::Release));
    let releases_only = SourceOptions {
        repositories: BTreeMap::from([(
            "web-*".to_owned(),
            Override {
                project: Some("web".to_owned()),
                ..Override::default()
            },
        )]),
        ..owner("a")
    };
    assert!(!releases_only.may_take(Signal::Tag));
    let tags_by_override = SourceOptions {
        repositories: BTreeMap::from([("core".to_owned(), signalled(&[Signal::Tag], None))]),
        ..owner("a")
    };
    assert!(tags_by_override.may_take(Signal::Tag));
    let no_releases = SourceOptions {
        signals: Some(vec![Signal::Tag]),
        ..releases_only
    };
    assert!(!no_releases.may_take(Signal::Release));
}

#[test]
fn signals_name_something_and_a_gitlab_source_tells_no_pre_release_by_a_flag() {
    let repository = |signals: Option<Vec<Signal>>, prerelease| SourceOptions {
        repository: Some("a/b".to_owned()),
        signals,
        prerelease,
        ..SourceOptions::default()
    };
    assert_eq!(
        repository(Some(Vec::new()), None).check(Forge::GitHub),
        Err("signals names no signal; give release, tag or both, or remove it".to_owned())
    );
    assert_eq!(
        repository(Some(vec![Signal::Tag]), Some(PrereleaseRule::Flag)).check(Forge::GitHub),
        Ok(())
    );
    assert_eq!(
        repository(None, Some(PrereleaseRule::Flag)).check(Forge::GitLab),
        Err("prerelease \"flag\" marks nothing on GitLab, whose releases carry no pre-release flag; choose either or tag".to_owned())
    );
    assert_eq!(
        repository(None, Some(PrereleaseRule::Tag)).check(Forge::GitLab),
        Ok(())
    );
    let overridden = |chosen: Override| SourceOptions {
        repositories: BTreeMap::from([("plugin-*".to_owned(), chosen)]),
        ..owner("a")
    };
    assert_eq!(
        overridden(signalled(&[], None)).check(Forge::GitHub),
        Err("repositories.\"plugin-*\".signals names no signal; give release, tag or both, or remove it".to_owned())
    );
    assert_eq!(
        overridden(signalled(&[Signal::Tag], Some(PrereleaseRule::Flag))).check(Forge::GitLab),
        Err("repositories.\"plugin-*\".prerelease \"flag\" marks nothing on GitLab, whose releases carry no pre-release flag; choose either or tag".to_owned())
    );
    assert_eq!(
        overridden(signalled(&[Signal::Tag], Some(PrereleaseRule::Flag))).check(Forge::Forgejo),
        Ok(())
    );
}

#[test]
fn two_overrides_as_narrow_as_each_other_tie_on_signals_and_prerelease_alone() {
    let both = |one: Override, other: Override| {
        SourceOptions {
            repositories: BTreeMap::from([
                ("plugin-*".to_owned(), one),
                ("*-sonarr".to_owned(), other),
            ]),
            ..owner("a")
        }
        .check(Forge::GitHub)
    };
    assert_eq!(
        both(
            signalled(&[Signal::Tag], None),
            signalled(&[Signal::Release], None)
        ),
        Err("repositories \"*-sonarr\" and \"plugin-*\" both set signals, are as narrow as each other and could both name one repository; make one narrower".to_owned())
    );
    let ruled = |prerelease| Override {
        prerelease: Some(prerelease),
        ..Override::default()
    };
    assert_eq!(
        both(ruled(PrereleaseRule::Tag), ruled(PrereleaseRule::Flag)),
        Err("repositories \"*-sonarr\" and \"plugin-*\" both set prerelease, are as narrow as each other and could both name one repository; make one narrower".to_owned())
    );
    assert_eq!(
        both(signalled(&[Signal::Tag], None), ruled(PrereleaseRule::Flag)),
        Ok(())
    );
}

#[test]
fn prefer_and_its_window_choose_between_a_tag_and_its_release_and_nothing_else() {
    let repository = |signals: Option<Vec<Signal>>| SourceOptions {
        repository: Some("a/b".to_owned()),
        signals,
        ..SourceOptions::default()
    };
    let both = || repository(Some(vec![Signal::Release, Signal::Tag]));
    let window = |text: &str| {
        Some(Duration(SignedDuration::from_secs(match text {
            "7d" => 7 * 86_400,
            "8d" => 8 * 86_400,
            _ => 600,
        })))
    };
    assert_eq!(both().prefer(), Prefer::Release);
    assert_eq!(both().tag_window(), TAG_WINDOW);
    assert_eq!(TAG_WINDOW, SignedDuration::from_hours(1));
    assert_eq!(both().held_for(&both().signals_of("b")), Some(TAG_WINDOW));
    assert_eq!(both().held_for(&Signals::default()), None);
    let tags_alone = Signals {
        taken: BTreeSet::from([Signal::Tag]),
        prerelease: PrereleaseRule::Either,
    };
    assert_eq!(both().held_for(&tags_alone), None);
    let first = SourceOptions {
        prefer: Some(Prefer::First),
        ..both()
    };
    assert_eq!(first.check(Forge::GitHub), Ok(()));
    assert_eq!(first.held_for(&both().signals_of("b")), None);
    let short = SourceOptions {
        tag_window: window("10m"),
        ..both()
    };
    assert_eq!(short.check(Forge::GitHub), Ok(()));
    assert_eq!(short.tag_window(), SignedDuration::from_mins(10));
    assert_eq!(
        SourceOptions {
            tag_window: window("7d"),
            ..both()
        }
        .check(Forge::GitHub),
        Ok(())
    );
    assert_eq!(
        SourceOptions {
            tag_window: window("8d"),
            ..both()
        }
        .check(Forge::GitHub),
        Err("tag_window is at most \"7d\"; shorten it".to_owned())
    );
    assert_eq!(
        SourceOptions {
            prefer: Some(Prefer::First),
            ..short.clone()
        }
        .check(Forge::GitHub),
        Err("tag_window is how long a tag waits for its Release, which prefer = \"first\" never does; remove one".to_owned())
    );
    for (key, options) in [
        (
            "prefer",
            SourceOptions {
                prefer: Some(Prefer::Release),
                ..repository(None)
            },
        ),
        (
            "tag_window",
            SourceOptions {
                tag_window: window("10m"),
                ..repository(Some(vec![Signal::Tag]))
            },
        ),
    ] {
        assert_eq!(
            options.check(Forge::GitHub),
            Err(format!(
                "{key} chooses between a tag and its Release, which no repository the source takes announces both of; give signals = [\"release\", \"tag\"], or remove {key}"
            ))
        );
    }
    assert_eq!(TAG_WINDOW_ALLOWED, SignedDuration::from_hours(168));
}

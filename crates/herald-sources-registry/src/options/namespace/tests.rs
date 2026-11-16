use herald_core::{Duration, SecretRef};

use super::*;

fn scoped() -> RegistryOptions {
    RegistryOptions {
        scope: Some("@acme".to_owned()),
        ..RegistryOptions::default()
    }
}

fn vendor() -> RegistryOptions {
    RegistryOptions {
        vendor: Some("acme".to_owned()),
        ..RegistryOptions::default()
    }
}

fn package() -> RegistryOptions {
    RegistryOptions {
        package: Some("left-pad".to_owned()),
        ..RegistryOptions::default()
    }
}

fn token() -> SecretRef {
    SecretRef {
        secret: "TOKEN".to_owned(),
    }
}

#[test]
fn a_crates_io_owner_is_a_user_or_a_github_team_as_crates_io_writes_them() {
    assert_eq!(
        CratesOwner::of("user:BurntSushi"),
        Some(CratesOwner::User("BurntSushi"))
    );
    assert_eq!(
        CratesOwner::of("team:github:rust-lang:libs"),
        Some(CratesOwner::Team("github:rust-lang:libs"))
    );
    for owner in [
        "BurntSushi",
        "user:",
        "user:a/b",
        "team:gitlab:rust-lang:libs",
        "team:github:rust-lang",
        "team:github::libs",
        "team:github:rust-lang:",
        "team:github:rust-lang:libs:more",
    ] {
        assert_eq!(CratesOwner::of(owner), None, "{owner}");
    }
}

#[test]
fn each_registry_names_its_namespace_by_its_own_key() {
    let all = RegistryOptions {
        scope: Some("@s".to_owned()),
        vendor: Some("v".to_owned()),
        owner: Some("user:o".to_owned()),
        namespace: Some("n".to_owned()),
        ..RegistryOptions::default()
    };
    assert_eq!(
        Registry::ALL.map(|registry| all.namespace_of(registry)),
        [Some("v"), Some("user:o"), Some("@s"), Some("n")]
    );
    assert_eq!(
        Registry::ALL.map(Registry::namespace_key),
        ["vendor", "owner", "scope", "namespace"]
    );
    assert_eq!(
        Registry::ALL.map(Registry::namespace_shape),
        [
            "the vendor's name, such as monolog",
            "user:<login> or team:github:<org>:<team>",
            "@scope",
            "the namespace's name, such as bitnami"
        ]
    );
    for (registry, other) in [
        (Registry::Packagist, "scope"),
        (Registry::CratesIo, "scope"),
        (Registry::Npm, "vendor"),
        (Registry::DockerHub, "scope"),
    ] {
        assert_eq!(
            all.check(registry),
            Err(format!(
                "the registry \"{}\" names a namespace by {}; remove {other}",
                registry.kind(),
                registry.namespace_key()
            ))
        );
    }
    let owner = RegistryOptions {
        owner: Some("user:o".to_owned()),
        ..RegistryOptions::default()
    };
    assert_eq!(
        owner.check(Registry::DockerHub),
        Err("the registry \"docker-hub\" names a namespace by namespace; remove owner".to_owned())
    );
}

#[test]
fn a_source_names_a_package_or_a_namespace_and_never_both() {
    assert_eq!(
        RegistryOptions::default().check(Registry::Npm),
        Err("give package, or scope to take every package of a namespace".to_owned())
    );
    assert_eq!(
        RegistryOptions {
            package: Some("left-pad".to_owned()),
            ..scoped()
        }
        .check(Registry::Npm),
        Err("package and scope name different things; give one of them".to_owned())
    );
    assert_eq!(scoped().check(Registry::Npm), Ok(()));
    assert_eq!(package().check(Registry::Npm), Ok(()));
}

#[test]
fn a_namespace_is_written_as_its_registry_writes_one() {
    let given = |registry: Registry, namespace: &str| {
        let mut options = RegistryOptions::default();
        match registry {
            Registry::Npm => options.scope = Some(namespace.to_owned()),
            Registry::Packagist => options.vendor = Some(namespace.to_owned()),
            Registry::CratesIo => options.owner = Some(namespace.to_owned()),
            Registry::DockerHub => options.namespace = Some(namespace.to_owned()),
        }
        options.check(registry)
    };
    for (registry, good, bad) in [
        (Registry::Npm, "@acme", "acme"),
        (Registry::Npm, "@acme", "@a/b"),
        (Registry::Packagist, "acme", "acme/x"),
        (Registry::CratesIo, "team:github:o:t", "o"),
        (Registry::DockerHub, "bitnami", "a b"),
    ] {
        assert_eq!(given(registry, good), Ok(()), "{good}");
        assert_eq!(
            given(registry, bad),
            Err(format!(
                "\"{bad}\" is not a {}; give it as {}",
                registry.namespace_key(),
                registry.namespace_shape()
            )),
            "{bad}"
        );
    }
}

#[test]
fn a_source_of_one_package_gives_no_key_that_chooses_among_a_namespace_s() {
    let keyed: [(&str, RegistryOptions); 8] = [
        (
            "include",
            RegistryOptions {
                include: vec!["a*".to_owned()],
                ..package()
            },
        ),
        (
            "exclude",
            RegistryOptions {
                exclude: vec!["a*".to_owned()],
                ..package()
            },
        ),
        (
            "most_listed",
            RegistryOptions {
                most_listed: NonZeroUsize::new(5),
                ..package()
            },
        ),
        (
            "most_members",
            RegistryOptions {
                most_members: NonZeroUsize::new(5),
                ..package()
            },
        ),
        (
            "keep_members",
            RegistryOptions {
                keep_members: Some(Duration(SignedDuration::from_hours(1))),
                ..package()
            },
        ),
        (
            "listed_page",
            RegistryOptions {
                listed_page: NonZeroUsize::new(5),
                ..package()
            },
        ),
        (
            "visibility",
            RegistryOptions {
                visibility: Some(Declared::Public),
                ..package()
            },
        ),
        (
            "unscoped",
            RegistryOptions {
                unscoped: true,
                ..package()
            },
        ),
    ];
    for (key, options) in keyed {
        assert_eq!(
            options.check(Registry::Npm),
            Err(format!(
                "{key} chooses among a namespace's packages; give scope instead of package, or remove {key}"
            ))
        );
    }
    for (key, options) in [
        (
            "listing",
            RegistryOptions {
                listing: Some(VendorListing::Composer),
                ..package()
            },
        ),
        (
            "listing_api",
            RegistryOptions {
                listing_api: Some("https://packagist.org".to_owned()),
                ..package()
            },
        ),
    ] {
        assert_eq!(
            options.check(Registry::Npm),
            Err(format!(
                "{key} is how a Packagist vendor is listed; give vendor on a packagist source, or remove {key}"
            ))
        );
    }
}

#[test]
fn the_keys_choosing_among_a_namespace_s_packages_are_bounded() {
    assert_eq!(
        RegistryOptions {
            include: vec!["[".to_owned()],
            ..scoped()
        }
        .check(Registry::Npm),
        Err(Glob::new("[").unwrap_err().to_string())
    );
    let most = |most: usize| RegistryOptions {
        most_members: NonZeroUsize::new(most),
        ..scoped()
    };
    assert_eq!(most(MOST_MEMBERS_ALLOWED).check(Registry::Npm), Ok(()));
    assert_eq!(
        most(MOST_MEMBERS_ALLOWED + 1).check(Registry::Npm),
        Err(format!(
            "most_members is at most {MOST_MEMBERS_ALLOWED}; lower it"
        ))
    );
    let paged = |page: usize| RegistryOptions {
        owner: Some("user:o".to_owned()),
        listed_page: NonZeroUsize::new(page),
        ..RegistryOptions::default()
    };
    assert_eq!(paged(100).check(Registry::CratesIo), Ok(()));
    assert_eq!(
        paged(101).check(Registry::CratesIo),
        Err("listed_page is at most 100; lower it".to_owned())
    );
    assert_eq!(
        RegistryOptions {
            listed_page: NonZeroUsize::new(5),
            ..scoped()
        }
        .check(Registry::Npm),
        Err("the registry \"npm\" lists a namespace in one answer; remove listed_page".to_owned())
    );
    assert_eq!(
        RegistryOptions {
            unscoped: true,
            ..vendor()
        }
        .check(Registry::Packagist),
        Err("unscoped is an npm scope's; remove it".to_owned())
    );
    assert_eq!(
        RegistryOptions {
            unscoped: true,
            ..scoped()
        }
        .check(Registry::Npm),
        Ok(())
    );
}

#[test]
fn a_packagist_vendor_is_listed_through_packagist_or_its_composer_repository() {
    let listed = |listing: Option<VendorListing>, api: Option<&str>| RegistryOptions {
        listing,
        listing_api: api.map(ToOwned::to_owned),
        ..vendor()
    };
    assert_eq!(listed(None, None).listing(), VendorListing::Packagist);
    assert_eq!(
        listed(Some(VendorListing::Composer), None).listing(),
        VendorListing::Composer
    );
    assert_eq!(
        listed(None, Some("https://packagist.example")).check(Registry::Packagist),
        Ok(())
    );
    assert_eq!(
        listed(Some(VendorListing::Composer), None).check(Registry::Packagist),
        Ok(())
    );
    assert_eq!(
        listed(Some(VendorListing::Composer), Some("https://packagist.example"))
            .check(Registry::Packagist),
        Err("a Composer repository is listed at api; remove listing_api, or set listing = \"packagist\"".to_owned())
    );
    assert_eq!(
        listed(None, Some("packagist")).check(Registry::Packagist),
        Err("\"packagist\" is not a web address; give the base URL that serves /packages/list.json, such as https://packagist.org".to_owned())
    );
    assert_eq!(
        RegistryOptions {
            listing: Some(VendorListing::Packagist),
            ..scoped()
        }
        .check(Registry::Npm),
        Err("listing is how a Packagist vendor is listed; give vendor on a packagist source, or remove listing".to_owned())
    );
}

#[test]
fn a_namespace_s_settings_default_as_owners_do_and_take_what_is_given() {
    let defaults = scoped();
    assert_eq!(defaults.most_listed(), MOST_LISTED);
    assert_eq!(defaults.most_members(), MOST_MEMBERS);
    assert_eq!(defaults.keep_members(), KEEP_MEMBERS);
    assert_eq!(defaults.listed_page(Registry::DockerHub), 100);
    assert_eq!(defaults.listed_page(Registry::Npm), usize::MAX);
    let given = RegistryOptions {
        most_listed: NonZeroUsize::new(7),
        most_members: NonZeroUsize::new(6),
        keep_members: Some(Duration(SignedDuration::from_hours(5))),
        listed_page: NonZeroUsize::new(4),
        ..scoped()
    };
    assert_eq!(given.most_listed().get(), 7);
    assert_eq!(given.most_members().get(), 6);
    assert_eq!(given.keep_members(), SignedDuration::from_hours(5));
    assert_eq!(given.listed_page(Registry::DockerHub), 4);
}

#[test]
fn a_namespace_s_packages_are_private_where_a_token_may_list_them_unless_declared_public() {
    assert_eq!(scoped().listed_visibility(), Visibility::Public);
    let tokened = RegistryOptions {
        token: Some(token()),
        ..scoped()
    };
    assert_eq!(tokened.listed_visibility(), Visibility::Private);
    assert_eq!(
        RegistryOptions {
            visibility: Some(Declared::Public),
            ..tokened
        }
        .listed_visibility(),
        Visibility::Public
    );
}

#[test]
fn a_namespace_s_selection_takes_its_globs_and_every_visibility() {
    let selection = RegistryOptions {
        include: vec!["a*".to_owned()],
        exclude: vec!["ab".to_owned()],
        ..scoped()
    }
    .selection()
    .unwrap();
    assert_eq!(selection.include, vec![Glob::new("a*").unwrap()]);
    assert_eq!(selection.exclude, vec![Glob::new("ab").unwrap()]);
    assert_eq!(
        selection.visibility,
        [
            Visibility::Public,
            Visibility::Internal,
            Visibility::Private
        ]
    );
    assert!(
        RegistryOptions {
            exclude: vec!["[".to_owned()],
            ..scoped()
        }
        .selection()
        .is_err()
    );
}

//! The keys that choose a source's packages.

use super::*;
use crate::options::packages::{MOST_PACKAGES, MOST_PACKAGES_ALLOWED};

#[test]
fn the_keys_that_choose_packages_are_given_only_where_packages_are_announced() {
    for forge in Forge::ALL {
        assert_eq!(packaged(SourceOptions::default()).check(forge), Ok(()));
    }
    let unpackaged = [
        (
            SourceOptions {
                package_types: Some(vec!["npm".to_owned()]),
                ..SourceOptions::default()
            },
            "package_types",
        ),
        (
            SourceOptions {
                packages: vec!["a".to_owned()],
                ..SourceOptions::default()
            },
            "packages",
        ),
        (
            SourceOptions {
                moving: Some(Vec::new()),
                ..SourceOptions::default()
            },
            "moving",
        ),
        (
            SourceOptions {
                registry: Some("registry.example".to_owned()),
                ..SourceOptions::default()
            },
            "registry",
        ),
        (
            SourceOptions {
                most_packages: NonZeroUsize::new(5),
                ..SourceOptions::default()
            },
            "most_packages",
        ),
        (
            SourceOptions {
                package_visibility: Some(crate::PackageVisibility::Package),
                ..SourceOptions::default()
            },
            "package_visibility",
        ),
    ];
    for (chosen, key) in unpackaged {
        assert_eq!(
            SourceOptions {
                repository: Some("a/b".to_owned()),
                ..chosen
            }
            .check(Forge::GitLab),
            Err(format!(
                "{key} chooses packages, which no repository the source takes announces; add \"package\" to signals, or remove {key}"
            ))
        );
    }
    assert_eq!(
        SourceOptions {
            token: None,
            ..packaged(SourceOptions::default())
        }
        .check(Forge::GitHub),
        Err("GitHub's packages API answers only with a token that may read packages; give token, or remove \"package\" from signals".to_owned())
    );
}

fn packaged(chosen: SourceOptions) -> SourceOptions {
    SourceOptions {
        repository: Some("a/b".to_owned()),
        signals: Some(vec![Signal::Package]),
        token: Some(SecretRef {
            secret: "GITHUB_TOKEN".to_owned(),
        }),
        ..chosen
    }
}

#[test]
fn package_types_globs_a_registry_and_the_most_listed_are_each_sound() {
    let typed = |types: &[&str]| {
        packaged(SourceOptions {
            package_types: Some(types.iter().map(|kind| (*kind).to_owned()).collect()),
            ..SourceOptions::default()
        })
    };
    assert_eq!(
        typed(&[]).check(Forge::Forgejo),
        Err("package_types names no type; give one, or remove it for every type".to_owned())
    );
    assert_eq!(typed(&GITHUB_PACKAGE_TYPES).check(Forge::GitHub), Ok(()));
    assert_eq!(
        typed(&["pypi"]).check(Forge::GitHub),
        Err("package_types \"pypi\" is not a type GitHub lists; give npm, maven, rubygems, docker, nuget, container".to_owned())
    );
    assert_eq!(
        typed(&["terraform_module", "ml-model", "pypi3"]).check(Forge::GitLab),
        Ok(())
    );
    for kind in ["", "PyPI", "a b"] {
        assert_eq!(
            typed(&[kind]).check(Forge::Forgejo),
            Err(format!(
                "package_types \"{kind}\" is not a type; name it as the forge does, in lowercase letters, digits, '_' and '-'"
            ))
        );
    }
    assert!(
        packaged(SourceOptions {
            packages: vec!["[".to_owned()],
            ..SourceOptions::default()
        })
        .check(Forge::GitLab)
        .unwrap_err()
        .starts_with("packages: ")
    );
    assert!(
        packaged(SourceOptions {
            moving: Some(vec!["[".to_owned()]),
            ..SourceOptions::default()
        })
        .check(Forge::GitLab)
        .unwrap_err()
        .starts_with("moving: ")
    );
    let registry = |registry: &str| {
        packaged(SourceOptions {
            registry: Some(registry.to_owned()),
            ..SourceOptions::default()
        })
        .check(Forge::Forgejo)
    };
    for sound in ["registry.example", "registry.example:5000"] {
        assert_eq!(registry(sound), Ok(()), "{sound}");
    }
    for unsound in [
        "registry",
        "https://registry.example",
        "registry.example:0",
        "registry.example:x",
        "Registry.example",
    ] {
        assert_eq!(
            registry(unsound),
            Err(format!(
                "registry \"{unsound}\" is not a registry's host; give it as registry.example or registry.example:5000"
            )),
            "{unsound}"
        );
    }
    let most = |most: usize| {
        packaged(SourceOptions {
            most_packages: NonZeroUsize::new(most),
            ..SourceOptions::default()
        })
        .check(Forge::GitLab)
    };
    assert_eq!(most(MOST_PACKAGES_ALLOWED), Ok(()));
    assert_eq!(
        most(MOST_PACKAGES_ALLOWED + 1),
        Err("most_packages is at most 100000; lower it".to_owned())
    );
    let defaults = SourceOptions::default();
    assert_eq!(defaults.most_packages(), MOST_PACKAGES);
    assert_eq!(defaults.moving().unwrap(), [Glob::new("latest").unwrap()]);
    assert_eq!(defaults.packages(), Ok(Vec::new()));
}

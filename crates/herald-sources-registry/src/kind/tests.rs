use herald_kinds::{Fault, SourceConfig, SourceKind};

use crate::Declared;

use super::*;

fn named(package: &str, token: Option<&str>) -> RegistryOptions {
    RegistryOptions {
        package: Some(package.to_owned()),
        token: token.map(|secret| SecretRef {
            secret: secret.to_owned(),
        }),
        backfill: 3,
        ..RegistryOptions::default()
    }
}

fn options(token: Option<&str>) -> RegistryOptions {
    named("left-pad", token)
}

#[test]
fn each_registry_is_a_kind_of_source_named_as_the_config_spells_it() {
    assert_eq!(
        [
            PackagistKind::NAME,
            CratesIoKind::NAME,
            NpmKind::NAME,
            DockerHubKind::NAME
        ],
        ["packagist", "crates-io", "npm", "docker-hub"]
    );
    for (checked, registry) in [
        (
            PackagistKind::checked(named("vendor/name", None)),
            Registry::Packagist,
        ),
        (CratesIoKind::checked(options(None)), Registry::CratesIo),
        (NpmKind::checked(options(Some("NPM_TOKEN"))), Registry::Npm),
        (
            DockerHubKind::checked(named("library/nginx", None)),
            Registry::DockerHub,
        ),
    ] {
        let source = checked.unwrap();
        assert_eq!(source.registry, registry);
        assert_eq!(source.kind(), registry.kind());
        assert_eq!(source.moving_tags(), registry.moving_tags());
        assert!(source.origins());
    }
}

#[test]
fn a_token_a_registry_does_not_take_keeps_the_source_out_at_its_key() {
    assert_eq!(
        CratesIoKind::checked(options(Some("TOKEN"))).unwrap_err(),
        Fault::at(
            "token",
            "the registry \"crates-io\" takes no token; remove token"
        )
    );
    let unnamed = RegistryOptions {
        package: Some(String::new()),
        ..options(None)
    };
    assert_eq!(
        NpmKind::checked(unnamed.clone()).unwrap_err(),
        Fault::here(unnamed.check(Registry::Npm).unwrap_err())
    );
}

#[test]
fn a_registry_source_names_its_token_and_polls_its_package() {
    let source = NpmKind::checked(options(Some("NPM_TOKEN"))).unwrap();
    assert_eq!(
        source.secret_refs(),
        vec![&SecretRef {
            secret: "NPM_TOKEN".to_owned()
        }]
    );
    assert_eq!(source.kinds(), &[Kind::PACKAGE_VERSION]);
    assert_eq!(source.backfill(), 3);
    assert_eq!(source.scheme(), None);
    let herald_kinds::Polling::One(poll) = source.poller(None, false) else {
        panic!("a source of one is polled as one");
    };
    assert_eq!(
        poll.api(),
        Package::new(Registry::Npm, "left-pad", &source.options).api()
    );
    assert_eq!(
        poll.remembered(),
        source.options.reading(Registry::Npm).remembered
    );
}

#[test]
fn a_registry_source_names_its_package_s_project() {
    assert_eq!(
        DockerHubKind::checked(named("library/nginx", None))
            .unwrap()
            .projects(),
        ["nginx"]
    );
    assert_eq!(
        PackagistKind::checked(named("vendor/name", None))
            .unwrap()
            .projects(),
        ["vendor/name"]
    );
}

#[test]
fn a_namespace_source_is_polled_as_its_packages_and_names_no_project() {
    let scope = |token: Option<&str>, visibility: Option<Declared>| RegistryOptions {
        package: None,
        scope: Some("@acme".to_owned()),
        visibility,
        ..options(token)
    };
    let source = NpmKind::checked(scope(None, None)).unwrap();
    assert_eq!(source.projects(), Vec::<String>::new());
    assert!(!source.unlisted());
    let herald_kinds::Polling::Many(expand) = source.poller(None, false) else {
        panic!("a namespace is polled as its packages");
    };
    assert_eq!(expand.api(), "https://registry.npmjs.org");
    assert!(
        NpmKind::checked(scope(Some("NPM_TOKEN"), None))
            .unwrap()
            .unlisted()
    );
    assert!(
        !NpmKind::checked(scope(Some("NPM_TOKEN"), Some(Declared::Public)))
            .unwrap()
            .unlisted()
    );
    let hub = |token: Option<&str>| RegistryOptions {
        package: None,
        namespace: Some("acme".to_owned()),
        username: token.map(|_| "acme".to_owned()),
        visibility: Some(Declared::Public),
        ..options(token)
    };
    assert!(!DockerHubKind::checked(hub(None)).unwrap().unlisted());
    assert!(DockerHubKind::checked(hub(Some("HUB"))).unwrap().unlisted());
    assert!(
        !NpmKind::checked(options(Some("NPM_TOKEN")))
            .unwrap()
            .unlisted()
    );
}

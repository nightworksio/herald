use herald_kinds::{Fault, SourceConfig, SourceKind};

use super::*;

fn named(package: &str, token: Option<&str>) -> RegistryOptions {
    RegistryOptions {
        package: package.to_owned(),
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
        package: String::new(),
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
    let poll = source.poller(None);
    assert_eq!(
        poll.api(),
        Package::new(Registry::Npm, &source.options).api()
    );
    assert_eq!(
        poll.remembered(),
        source.options.reading(Registry::Npm).remembered
    );
}

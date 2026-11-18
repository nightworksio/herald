use herald_core::SecretRef;
use herald_kinds::{Bound, Fault, Polling, Remembers, SourceConfig, SourceKind};
use herald_ports::{Request, Response, Secret};

use super::*;
use crate::Declared;

mod catalog;
mod image;
mod options;
mod session;
mod wire;

/// When the registry answers, in each answer's `Date`.
pub(super) const DATE: &str = "Tue, 17 Nov 2026 12:00:00 GMT";

/// An answer of `status` with `body`, dated [`DATE`].
pub(super) fn answer(status: u16, body: &str) -> Response {
    Response {
        status,
        headers: vec![("Date".to_owned(), DATE.to_owned())],
        body: body.as_bytes().to_vec(),
    }
}

/// `response` with the header `name` set to `value`.
pub(super) fn with(mut response: Response, name: &str, value: &str) -> Response {
    response.headers.push((name.to_owned(), value.to_owned()));
    response
}

/// The value of `request`'s header `name`, as its exposed text.
pub(super) fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(held, _)| held.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.expose())
}

/// A token named `TOKEN`.
pub(super) fn token() -> SecretRef {
    SecretRef {
        secret: "TOKEN".to_owned(),
    }
}

/// The token named `TOKEN`, read as `secret`.
pub(super) fn read_token() -> Secret {
    Secret::new("TOKEN", "secret")
}

/// The options of a source of `repository` on the registry at `registry`.
pub(super) fn of(registry: &str, repository: &str) -> OciOptions {
    OciOptions {
        registry: registry.to_owned(),
        repository: Some(repository.to_owned()),
        ..OciOptions::default()
    }
}

/// The options of a source of a registry's catalog at `registry`.
pub(super) fn catalogued(registry: &str) -> OciOptions {
    OciOptions {
        registry: registry.to_owned(),
        catalog: true,
        ..OciOptions::default()
    }
}

#[test]
fn an_oci_source_is_a_kind_named_as_the_config_spells_it_and_refused_at_the_key_at_fault() {
    assert_eq!(OciKind::NAME, "oci");
    let source = OciKind::checked(of("https://registry.example", "acme/app")).unwrap();
    assert_eq!(source.options, of("https://registry.example", "acme/app"));
    assert_eq!(
        OciKind::checked(OciOptions::default()).unwrap_err(),
        Fault::at(
            "registry",
            "\"\" is not a web address; give the registry's base URL, such as https://registry.example"
        )
    );
}

#[test]
fn an_oci_source_announces_package_versions_with_their_origins_and_backfill() {
    let source = OciSource::new(OciOptions {
        token: Some(token()),
        username: Some("me".to_owned()),
        backfill: 2,
        ..of("https://registry.example", "acme/app")
    });
    assert_eq!(source.kind(), "oci");
    assert_eq!(source.token(), Some(&token()));
    assert_eq!(source.kinds(), &[herald_core::Kind::PACKAGE_VERSION]);
    assert_eq!(source.backfill(), 2);
    assert!(source.origins());
    assert!(!source.moving_tags());
    assert_eq!(source.standing(), None);
}

#[test]
fn an_oci_source_may_take_private_repositories_where_its_token_reads_them_undeclared() {
    let anonymous = of("https://registry.example", "acme/app");
    let tokened = OciOptions {
        token: Some(token()),
        username: Some("me".to_owned()),
        ..anonymous.clone()
    };
    let declared = OciOptions {
        visibility: Some(Declared::Public),
        ..tokened.clone()
    };
    assert!(!OciSource::new(anonymous).unlisted());
    assert!(OciSource::new(tokened).unlisted());
    assert!(!OciSource::new(declared).unlisted());
}

#[test]
fn a_repository_source_announces_as_its_repository_and_a_catalog_names_no_project_ahead() {
    let hub = OciSource::new(of("https://registry-1.docker.io", "caddy"));
    assert_eq!(hub.projects(), ["caddy"]);
    let named = OciSource::new(of("https://registry.example", "acme/app"));
    assert_eq!(named.projects(), ["acme/app"]);
    let catalog = OciSource::new(catalogued("https://registry.example"));
    assert_eq!(catalog.projects(), Vec::<String>::new());
}

#[test]
fn an_oci_cursor_remembers_every_tag_each_as_long_as_its_identity_may_grow() {
    let one = OciSource::new(of("https://registry.example", "acme/app"));
    assert_eq!(
        one.remembers(0),
        Some(Remembers {
            identities: 10_000,
            each: "registry.example".len() + 1 + "acme/app".len() + 1 + 128 + 5,
            key: "most_tags",
            bound: Bound::Default,
            per: 1,
            beside: 0,
            besides: &[],
        })
    );
    let hub = OciSource::new(OciOptions {
        remembered: std::num::NonZeroUsize::new(20_000),
        ..of("https://registry-1.docker.io", "caddy")
    });
    assert_eq!(
        hub.remembers(0),
        Some(Remembers {
            identities: 20_000,
            each: "docker.io".len() + 1 + "library/caddy".len() + 1 + 128 + 5,
            key: "remembered",
            bound: Bound::Named,
            per: 1,
            beside: 0,
            besides: &[],
        })
    );
    let catalog = OciSource::new(catalogued("https://registry.example"));
    assert_eq!(
        catalog.remembers(0).map(|remembers| remembers.each),
        Some("registry.example".len() + 1 + 255 + 1 + 128 + 5)
    );
}

#[test]
fn a_repository_is_polled_as_one_and_a_catalog_as_each_repository_at_the_registry() {
    let one = OciSource::new(of("https://registry.example/", "acme/app"));
    let Polling::One(poll) = one.poller(None, false) else {
        panic!("a repository is polled as one");
    };
    assert_eq!(poll.api(), "https://registry.example");
    assert_eq!(poll.bases(), &[] as &[String]);
    assert_eq!(poll.remembered(), 10_000);
    let hub = OciSource::new(of("https://registry-1.docker.io", "caddy"));
    assert_eq!(
        hub.poller(None, false).base(),
        herald_ports::Base::Among(&[
            "https://registry-1.docker.io".to_owned(),
            "https://auth.docker.io".to_owned(),
            "https://production.cloudfront.docker.com".to_owned(),
            "https://production.cloudflare.docker.com".to_owned(),
        ])
    );
    let catalog = OciSource::new(catalogued("https://registry.example"));
    let Polling::Many(expand) =
        catalog.poller(Some(herald_kinds::Credential::Named(read_token())), false)
    else {
        panic!("a catalog is polled as each repository");
    };
    assert_eq!(expand.api(), "https://registry.example");
    assert_eq!(expand.owner_key(), "catalog");
}

#[test]
fn a_source_naming_no_bound_is_fitted_to_its_store_and_one_naming_its_tags_is_named() {
    let unset = OciSource::new(of("https://registry.example", "acme/app"));
    let holding = |tags: usize| Remembers::REST + tags * unset.remembers(0).unwrap().each;
    let fitted = unset.fitted(holding(500) + 1, 0).unwrap();
    let remembers = fitted.remembers(0).unwrap();
    assert_eq!(remembers.identities, 500);
    assert_eq!(remembers.bound, Bound::Fitted);
    assert_eq!(remembers.key, "most_tags");
    let Some(source) = fitted.of::<OciSource>() else {
        panic!("a fitted oci source is one");
    };
    assert!(source.fitted);
    assert_eq!(source.options.most_tags, std::num::NonZeroUsize::new(500));
    assert_eq!(unset.fitted(0, 0), None);
    assert_eq!(unset.fitted(holding(30) - 1, 0), None);
    assert!(unset.fitted(holding(30), 0).is_some());
    let named = OciSource::new(OciOptions {
        most_tags: std::num::NonZeroUsize::new(100),
        ..of("https://registry.example", "acme/app")
    });
    assert_eq!(named.remembers(0).unwrap().bound, Bound::Named);
}

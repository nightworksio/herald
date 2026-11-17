//! Whatever a registry sends, its parser answers versions or an error, and
//! each version's identity names the registry and the package and ends with
//! the version; a namespace's listing answers packages of the namespace, a
//! cap's refusal, or an error. An OCI registry answering the fuzzed bytes as
//! its tags, manifests, configs, catalog or challenge gives tags of the
//! image it was asked for, repositories with projects, or an error.

#![no_main]

use std::num::NonZeroUsize;

use herald_kinds::{Expanded, Polling, SourceConfig as _, Unlisted};
use herald_ports::{Http, HttpError, Request, Response};
use herald_sources_registry::{
    OciOptions, OciSource, Package, Registry, RegistryOptions, RegistrySource,
};
use libfuzzer_sys::fuzz_target;

const REGISTRIES: [Registry; 4] = [
    Registry::Packagist,
    Registry::CratesIo,
    Registry::Npm,
    Registry::DockerHub,
];

/// The most packages a fuzzed listing reads, so an endless one ends soon.
const MOST_LISTED: NonZeroUsize = NonZeroUsize::new(50).unwrap();

/// An `Http` answering every request with the fuzzed bytes.
struct Answering(Vec<u8>);

impl Http for Answering {
    async fn send(&self, _: Request) -> Result<Response, HttpError> {
        Ok(Response {
            status: 200,
            headers: Vec::new(),
            body: self.0.clone(),
        })
    }
}

/// An `Http` refusing every request with the fuzzed bytes as its challenge.
struct Challenging(String);

impl Http for Challenging {
    async fn send(&self, _: Request) -> Result<Response, HttpError> {
        Ok(Response {
            status: 401,
            headers: vec![("WWW-Authenticate".to_owned(), self.0.clone())],
            body: Vec::new(),
        })
    }
}

/// The options of an oci source of `fuzzed/image`, reading every tag it
/// finds, or of the registry's catalog.
fn oci(catalog: bool) -> OciSource {
    OciSource::new(OciOptions {
        registry: "https://registry.example".to_owned(),
        repository: (!catalog).then(|| "fuzzed/image".to_owned()),
        catalog,
        backfill: 1,
        most_tags: Some(MOST_LISTED),
        most_listed: catalog.then_some(MOST_LISTED),
        ..OciOptions::default()
    })
}

/// What an oci source makes of a registry answering `bytes` to every
/// request, and of `bytes` as its challenge.
fn registered(bytes: &[u8]) {
    let Polling::One(image) = oci(false).poller(None, false) else {
        panic!("a repository is polled as one");
    };
    let challenging = Challenging(String::from_utf8_lossy(bytes).into_owned());
    for http in [&Answering(bytes.to_vec()) as &dyn herald_ports::DynHttp, &challenging] {
        if let Ok(Some(polled)) = pollster::block_on(image.polled(http, None, jiff::Timestamp::UNIX_EPOCH)) {
            for event in &polled.events {
                let identity = event.identity().as_str();
                assert!(identity.starts_with("registry.example/fuzzed/image/"), "{identity}");
                assert!(identity.ends_with(event.version().unwrap_or_default()), "{identity}");
            }
        }
        let _ = pollster::block_on(image.probed(http));
    }
    let Polling::Many(catalog) = oci(true).poller(None, false) else {
        panic!("a catalog is polled as its repositories");
    };
    if let Ok(Expanded::Listed { members, .. }) =
        pollster::block_on(catalog.listed(&Answering(bytes.to_vec()), None))
    {
        assert!(members.len() <= MOST_LISTED.get());
        for member in &members {
            assert!(catalog.project(member).is_ok(), "{member:?}");
            assert!(!catalog.origin(member).repository.is_empty(), "{member:?}");
        }
    }
}

/// What each registry's namespace listing makes of `bytes`.
fn listed(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    for registry in REGISTRIES {
        let mut options = RegistryOptions {
            most_listed: Some(MOST_LISTED),
            unscoped: true,
            ..RegistryOptions::default()
        };
        match registry {
            Registry::Npm => options.scope = Some("@fuzzed".to_owned()),
            Registry::Packagist => options.vendor = Some("fuzzed".to_owned()),
            Registry::CratesIo => options.owner = Some("user:fuzzed".to_owned()),
            Registry::DockerHub => options.namespace = Some("fuzzed".to_owned()),
        }
        let source = RegistrySource { registry, options };
        let Polling::Many(expand) = source.poller(None, false) else {
            panic!("a namespace is polled as its packages");
        };
        match pollster::block_on(expand.listed(&http, None)) {
            Ok(Expanded::Listed { members, .. }) => {
                assert!(members.len() <= MOST_LISTED.get());
                for member in &members {
                    assert!(expand.project(member).is_ok(), "{member:?}");
                }
            }
            Ok(Expanded::Unchanged) => panic!("an unconditional listing is never unchanged"),
            Err(Unlisted::Over(_) | Unlisted::Failed(_)) => {}
        }
    }
}

fuzz_target!(|bytes: &[u8]| {
    let response = Response {
        status: 200,
        headers: Vec::new(),
        body: bytes.to_vec(),
    };
    let options = RegistryOptions::default();
    for registry in REGISTRIES {
        let package = Package::new(registry, "fuzzed/package", &options);
        if let Ok(polled) = package.versions(&response) {
            assert!(polled.events.len() <= 30);
            for event in &polled.events {
                let identity = event.identity().as_str();
                assert!(identity.contains("/fuzzed/package/"), "{identity}");
                assert!(
                    identity.ends_with(event.version().unwrap_or_default()),
                    "{identity}"
                );
            }
        }
    }
    listed(bytes);
    registered(bytes);
});

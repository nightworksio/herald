//! Whatever a registry sends, its parser answers versions or an error, and
//! each version's identity names the registry and the package and ends with
//! the version; a namespace's listing answers packages of the namespace, a
//! cap's refusal, or an error.

#![no_main]

use std::num::NonZeroUsize;

use herald_kinds::{Expanded, Polling, SourceConfig as _, Unlisted};
use herald_ports::{Http, HttpError, Request, Response};
use herald_sources_registry::{Package, Registry, RegistryOptions, RegistrySource};
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
});

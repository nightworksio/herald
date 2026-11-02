//! Whatever a registry sends, its parser answers versions or an error, and
//! each version's identity names the registry and the package and ends with
//! the version.

#![no_main]

use herald_ports::Response;
use herald_sources_registry::{Package, Registry, RegistryOptions};
use libfuzzer_sys::fuzz_target;

const REGISTRIES: [Registry; 4] = [
    Registry::Packagist,
    Registry::CratesIo,
    Registry::Npm,
    Registry::DockerHub,
];

fuzz_target!(|bytes: &[u8]| {
    let response = Response { status: 200, headers: Vec::new(), body: bytes.to_vec() };
    let options = RegistryOptions { package: "fuzzed/package".to_owned(), ..RegistryOptions::default() };
    for registry in REGISTRIES {
        let package = Package::new(registry, &options);
        if let Ok(polled) = package.versions(&response) {
            assert!(polled.events.len() <= 30);
            for event in &polled.events {
                let identity = event.identity().as_str();
                assert!(identity.contains("/fuzzed/package/"), "{identity}");
                assert!(identity.ends_with(event.version().unwrap_or_default()), "{identity}");
            }
        }
    }
});

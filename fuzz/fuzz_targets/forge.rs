//! Whatever a forge sends, its parsers answer an event, nothing or an error,
//! and an event's identity names its repository and ends with its version.

#![no_main]

use herald_core::Event;
use herald_ports::Response;
use herald_sources_forge::{GitHub, Releases, Repository};
use libfuzzer_sys::fuzz_target;

fn identified(event: &Event, id: Option<u64>) {
    let identity = event.identity().as_str();
    let version = event.version().unwrap_or_default();
    assert!(identity.ends_with(version), "{event:?}");
    if let Some(id) = id {
        assert!(identity.contains(&format!("/{id}/")), "{event:?}");
    }
}

fuzz_target!(|bytes: &[u8]| {
    if let Ok(Some(event)) = GitHub::event(bytes) {
        identified(&event, None);
    }
    let response = Response { status: 200, headers: Vec::new(), body: bytes.to_vec() };
    let _ = GitHub::repository(&response);
    let repository = Repository { id: 7, name: "fuzzed".to_owned() };
    if let Ok(Some(event)) = GitHub::release(&repository, &response) {
        identified(&event, Some(repository.id));
    }
    if let Ok(Releases { events, .. }) = GitHub::releases(&repository, &response) {
        for event in &events {
            identified(event, Some(repository.id));
        }
    }
});

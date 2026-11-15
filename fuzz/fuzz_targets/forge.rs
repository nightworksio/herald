//! Whatever a forge sends, its parsers answer an event, nothing or an error,
//! and an event's identity names its repository and ends with its version;
//! an owner's webhook names the member its event's identity names.

#![no_main]

use herald_core::{Event, Polled, PrereleaseRule, Selection};
use herald_ports::Response;
use herald_sources_forge::{Forge, Hooked, Repository, Signals};
use herald_wire::PARSE_LIMIT;
use libfuzzer_sys::fuzz_target;

const FORGES: [Forge; 3] = [Forge::GitHub, Forge::GitLab, Forge::Forgejo];

fn identified(event: &Event, id: Option<u64>) {
    let identity = event.identity().as_str();
    let version = event.version().unwrap_or_default();
    assert!(identity.ends_with(version), "{event:?}");
    if let Some(id) = id {
        assert!(identity.contains(&format!("/{id}/")), "{event:?}");
    }
}

fuzz_target!(|bytes: &[u8]| {
    let response = Response {
        status: 200,
        headers: Vec::new(),
        body: bytes.to_vec(),
    };
    let repository = Repository {
        id: 7,
        name: "fuzzed".to_owned(),
        page: None,
    };
    let rule = PrereleaseRule::default();
    for forge in FORGES {
        if let Ok(Some(event)) = forge.event(bytes, rule) {
            identified(&event, None);
        }
        let _ = forge.repository(&response);
        if let Ok(Some(event)) = forge.release(&repository, &response, rule) {
            identified(&event, Some(repository.id));
        }
        if let Ok(Polled { events, .. }) =
            forge.releases(&repository, &response, (PARSE_LIMIT, rule))
        {
            for event in &events {
                identified(event, Some(repository.id));
            }
        }
        let headers = [
            ("X-GitHub-Event", "release"),
            ("X-Gitlab-Event", "Release Hook"),
            ("X-Forgejo-Event", "release"),
        ];
        if let Ok(Hooked::Member { event, id, .. }) = forge.hooked_owned(
            (&headers, bytes),
            ("fuzzed", true),
            (&Selection::default(), &|_| Signals::default()),
        ) {
            identified(&event, None);
            assert!(
                event.identity().as_str().contains(&format!("/{id}/")),
                "{event:?}"
            );
        }
    }
});

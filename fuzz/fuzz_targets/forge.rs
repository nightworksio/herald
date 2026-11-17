//! Whatever a forge sends, its parsers answer an event, nothing or an error,
//! and an event's identity names its repository and ends with its version;
//! an owner's webhook names the member its event's identity names, and its
//! events, feed and GraphQL answers name members by their numeric ids; a
//! ghcr source's listing names images with a project, and each version's
//! identity names its registry, owner and image.

#![no_main]

use std::collections::BTreeMap;

use herald_core::{
    Event, EventsRead, Listed, Owner, Polled, PrereleaseRule, SecretRef, Selection, Visibility,
};
use herald_kinds::{Changed, Credential, Polling, Rounded, SourceConfig as _};
use herald_ports::{Http, HttpError, Request, Response, Secret};
use herald_sources_forge::{Forge, Hooked, Repository, Signals};
use herald_sources_forge::{ForgeSource, GhcrOptions, GhcrSource, SourceOptions, Transport};
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

/// The owner sources whose events, feed or GraphQL batches read a whole
/// owner's answer at once.
const OWNERS: [(Forge, Transport); 4] = [
    (Forge::GitHub, Transport::Events),
    (Forge::Forgejo, Transport::Feed),
    (Forge::GitHub, Transport::Graphql),
    (Forge::GitLab, Transport::Graphql),
];

/// What each owner source makes of `bytes` as its events, feed or batch.
fn owned(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    let member = Listed {
        id: "7".to_owned(),
        name: "fuzzed".to_owned(),
        path: "fuzzer/fuzzed".to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    for (forge, transport) in OWNERS {
        let source = ForgeSource {
            forge,
            options: SourceOptions {
                owner: Some("fuzzer".to_owned()),
                transport: Some(transport),
                token: Some(SecretRef {
                    secret: "TOKEN".to_owned(),
                }),
                ..SourceOptions::default()
            },
            names: None,
        };
        let Polling::Many(expand) =
            source.poller(Some(Credential::Named(Secret::new("TOKEN", "t"))), false)
        else {
            continue;
        };
        let read = EventsRead::default();
        let changed = pollster::block_on(expand.changed(&http, Owner::Organization, Some(&read)));
        if let Some(Ok(Changed::Named { ids, .. })) = changed {
            assert!(ids.iter().all(|id| id.parse::<u64>().is_ok()), "{ids:?}");
        }
        let members = [(&member, None)];
        for polled in pollster::block_on(expand.polled(&http, &members, &Rounded::Nothing))
            .into_iter()
            .flatten()
            .flatten()
        {
            for event in &polled.events {
                identified(event, None);
            }
        }
    }
}

/// What a ghcr source makes of `bytes` as its owner's listing and as an
/// image's versions: images with a project, and versions whose identity
/// names the registry, the owner and the image and ends with the tag.
fn images(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    let source = GhcrSource {
        options: GhcrOptions {
            owner: "fuzzer".to_owned(),
            token: SecretRef {
                secret: "TOKEN".to_owned(),
            },
            api: None,
            registry: None,
            include: Vec::new(),
            exclude: Vec::new(),
            visibility: Some(vec![
                Visibility::Public,
                Visibility::Internal,
                Visibility::Private,
            ]),
            moving: None,
            package_visibility: None,
            most_packages: None,
            keep_members: None,
            project: None,
            backfill: 0,
            page: None,
            remembered: None,
            parse: None,
        },
    };
    let Polling::Many(expand) =
        source.poller(Some(Credential::Named(Secret::new("TOKEN", "t"))), false)
    else {
        panic!("an owner's images are polled as members");
    };
    if let Ok(listed) = pollster::block_on(expand.listed(&http, None)) {
        for image in listed.members() {
            assert!(expand.project(&image).is_ok(), "{image:?}");
            assert_eq!(expand.origin(&image).visibility, image.visibility);
        }
    }
    let image = Listed {
        id: "7".to_owned(),
        name: "fuzzed".to_owned(),
        path: "fuzzed".to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    if let Ok(Some(polled)) = pollster::block_on(expand.member(&image).polled(&http, None)) {
        for event in &polled.events {
            identified(event, None);
            assert!(
                event
                    .identity()
                    .as_str()
                    .starts_with("ghcr.io/fuzzer/fuzzed/"),
                "{event:?}"
            );
        }
    }
}

fuzz_target!(|bytes: &[u8]| {
    owned(bytes);
    images(bytes);
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

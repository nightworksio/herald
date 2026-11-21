//! Whatever a forge sends, its parsers answer an event, nothing or an error,
//! and an event's identity names its repository and ends with its version;
//! an owner's webhook names the member its event's identity names, and its
//! events, feed and GraphQL answers name members by their numeric ids; a
//! ghcr source's listing names images with a project, and each version's
//! identity names its registry, owner and image; a run, delivered or
//! polled, is named as a run of its repository's id; and an alert,
//! delivered or polled, is named as an alert of its repository's id and
//! carries only the attributes an alert carries; a pull request,
//! delivered or polled, is named as a pull request of its repository's id,
//! and an issue as an issue of its repository's id; a repository's open
//! pull requests read for a view are its own, as many as the view lists;
//! and a bump pattern reads a package its title writes.

#![no_main]

use std::collections::BTreeMap;

use herald_core::Signal;
use herald_core::{
    Event, EventsRead, Listed, Owner, Polled, PrereleaseRule, SecretRef, Selection, Visibility,
};
use herald_kinds::{Changed, Credential, Polling, Rounded, SourceConfig as _};
use herald_ports::{Http, HttpError, Request, Response, Secret};
use herald_sources_forge::{
    AlertAction, Alerts, ForgeSource, GhcrOptions, GhcrSource, Pulls, RunFrom, Runs, SourceOptions,
    Transport, ci_run,
};
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
        for polled in pollster::block_on(expand.polled(
            &http,
            &members,
            &Rounded::Nothing,
            jiff::Timestamp::UNIX_EPOCH,
        ))
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
    if let Ok(Some(polled)) = pollster::block_on(expand.member(&image).polled(
        &http,
        None,
        jiff::Timestamp::UNIX_EPOCH,
    )) {
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

/// That `event` is a run of the repository of id `id`, delivered or
/// polled.
fn ran(event: &Event, id: &str) {
    let identity = event.identity().as_str();
    assert!(
        ["/run/", "/check-suite/", "/status/"]
            .iter()
            .any(|ran| identity.contains(&format!("/{id}{ran}"))),
        "{event:?}"
    );
}

/// What a source announcing every run of every forge makes of `bytes` as
/// a run's delivery, a CI run's event and its runs' listings.
fn runs(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    let headers = [
        ("X-GitHub-Event", "workflow_run"),
        ("X-GitHub-Event", "check_suite"),
        ("X-GitHub-Event", "status"),
        ("X-Gitlab-Event", "Pipeline Hook"),
        ("X-Forgejo-Event", "workflow_run_success"),
        ("X-Gitea-Event", "action_run_failure"),
    ];
    for forge in FORGES {
        let from = if forge == Forge::GitHub {
            vec![RunFrom::Actions, RunFrom::Checks]
        } else {
            vec![RunFrom::Actions]
        };
        let options = SourceOptions {
            repository: Some("fuzzer/fuzzed".to_owned()),
            signals: Some(vec![Signal::Run]),
            runs: Some(Runs {
                from: Some(from),
                ..Runs::default()
            }),
            ..SourceOptions::default()
        };
        let source = ForgeSource {
            forge,
            options: options.clone(),
            names: None,
        };
        for header in headers {
            if let Ok(Hooked::Event(event)) = source.hooked(&[header], bytes) {
                let origin = event.origin().expect("a delivered run names its origin");
                ran(&event, &origin.id);
            }
            if let Ok(Some(event)) = ci_run((forge, &options), (header.1, bytes)) {
                let origin = event.origin().expect("a CI run's event names its origin");
                ran(&event, &origin.id);
            }
        }
        if let Polling::One(poll) = source.poller(None, false)
            && let Ok(Some(polled)) =
                pollster::block_on(poll.polled(&http, None, jiff::Timestamp::UNIX_EPOCH))
        {
            for event in &polled.events {
                let origin = event.origin().expect("a polled run names its origin");
                ran(event, &origin.id);
            }
        }
    }
}

/// The attributes an alert's event may carry.
const ALERTED: [&str; 10] = [
    "action",
    "ecosystem",
    "number",
    "package",
    "path",
    "resolution",
    "rule",
    "severity",
    "signal",
    "tool",
];

/// That `event` is an alert of the repository of id `id`, delivered or
/// polled, carrying only what an alert carries.
fn alerted(event: &Event, id: &str) {
    assert!(
        event.identity().as_str().contains(&format!("/{id}/alert/")),
        "{event:?}"
    );
    assert!(
        event
            .attributes()
            .keys()
            .all(|name| ALERTED.contains(&name.as_str())),
        "{event:?}"
    );
}

/// What a source announcing every alert of GitHub and GitLab makes of
/// `bytes` as an alert's delivery and its alerts' listings.
fn alerts(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    let headers = [
        ("X-GitHub-Event", "code_scanning_alert"),
        ("X-GitHub-Event", "dependabot_alert"),
        ("X-GitHub-Event", "secret_scanning_alert"),
        ("X-Gitlab-Event", "Vulnerability Hook"),
    ];
    for forge in [Forge::GitHub, Forge::GitLab] {
        let source = ForgeSource {
            forge,
            options: SourceOptions {
                repository: Some("fuzzer/fuzzed".to_owned()),
                signals: Some(vec![Signal::Alert]),
                alerts: Some(Alerts {
                    actions: Some(vec![
                        AlertAction::Created,
                        AlertAction::Reopened,
                        AlertAction::Confirmed,
                        AlertAction::Fixed,
                        AlertAction::Dismissed,
                    ]),
                    ..Alerts::default()
                }),
                ..SourceOptions::default()
            },
            names: None,
        };
        for header in headers {
            if let Ok(Hooked::Event(event)) = source.hooked(&[header], bytes) {
                let origin = event.origin().expect("a delivered alert names its origin");
                alerted(&event, &origin.id);
            }
        }
        if let Polling::One(poll) = source.poller(None, false)
            && let Ok(Some(polled)) =
                pollster::block_on(poll.polled(&http, None, jiff::Timestamp::UNIX_EPOCH))
        {
            for event in &polled.events {
                let origin = event.origin().expect("a polled alert names its origin");
                alerted(event, &origin.id);
            }
        }
    }
}

/// What a source announcing every pull request of every forge makes of
/// `bytes` as a pull request's delivery and its listing.
fn pulls(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    let headers = [
        ("X-GitHub-Event", "pull_request"),
        ("X-Gitlab-Event", "Merge Request Hook"),
        ("X-Forgejo-Event", "pull_request"),
    ];
    for forge in FORGES {
        let source = ForgeSource {
            forge,
            options: SourceOptions {
                repository: Some("fuzzer/fuzzed".to_owned()),
                signals: Some(vec![Signal::PullRequest]),
                pulls: Some(Pulls {
                    drafts: Some(true),
                    ..Pulls::default()
                }),
                ..SourceOptions::default()
            },
            names: None,
        };
        let pulled = |event: &Event, id: &str| {
            assert!(
                event.identity().as_str().contains(&format!("/{id}/pull/")),
                "{event:?}"
            );
        };
        for header in headers {
            if let Ok(Hooked::Event(event)) = source.hooked(&[header], bytes) {
                let origin = event
                    .origin()
                    .expect("a delivered pull request names its origin");
                pulled(&event, &origin.id);
            }
        }
        if let Polling::One(poll) = source.poller(None, false)
            && let Ok(Some(polled)) =
                pollster::block_on(poll.polled(&http, None, jiff::Timestamp::UNIX_EPOCH))
        {
            for event in &polled.events {
                let origin = event
                    .origin()
                    .expect("a polled pull request names its origin");
                pulled(event, &origin.id);
            }
        }
    }
}

/// What each forge's deliveries kept mode folds make of `bytes`, folded
/// in turn into one record (ADR-0061).
fn folded(bytes: &[u8]) {
    let headers = [
        ("X-GitHub-Event", "pull_request"),
        ("X-GitHub-Event", "pull_request_review"),
        ("X-GitHub-Event", "issue_comment"),
        ("X-GitHub-Event", "check_run"),
        ("X-GitHub-Event", "status"),
        ("X-Gitlab-Event", "Merge Request Hook"),
        ("X-Gitlab-Event", "Pipeline Hook"),
        ("X-Gitlab-Event", "Note Hook"),
        ("X-Forgejo-Event", "pull_request_approved"),
    ];
    for forge in FORGES {
        let source = ForgeSource {
            forge,
            options: SourceOptions {
                repository: Some("fuzzer/fuzzed".to_owned()),
                ..SourceOptions::default()
            },
            names: None,
        };
        for header in headers {
            let Ok(Some(folding)) = source.folded(&[header], bytes) else {
                continue;
            };
            let mut kept: Option<herald_core::KeptPull> = None;
            for folded in &folding.folded {
                let key = herald_core::PullKey {
                    repository: folding.repository.clone(),
                    number: folded.number().unwrap_or_default(),
                };
                if let Some(record) = herald_core::KeptPull::fold(kept.clone(), key, folded) {
                    kept = Some(record);
                }
            }
        }
    }
}

/// What a source announcing every issue of every forge makes of
/// `bytes` as a issue's delivery and its listing.
fn issues(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    let headers = [
        ("X-GitHub-Event", "issues"),
        ("X-Gitlab-Event", "Issue Hook"),
        ("X-Forgejo-Event", "issues"),
    ];
    for forge in FORGES {
        let source = ForgeSource {
            forge,
            options: SourceOptions {
                repository: Some("fuzzer/fuzzed".to_owned()),
                signals: Some(vec![Signal::Issue]),
                ..SourceOptions::default()
            },
            names: None,
        };
        let issued = |event: &Event, id: &str| {
            assert!(
                event.identity().as_str().contains(&format!("/{id}/issue/")),
                "{event:?}"
            );
        };
        for header in headers {
            if let Ok(Hooked::Event(event)) = source.hooked(&[header], bytes) {
                let origin = event.origin().expect("a delivered issue names its origin");
                issued(&event, &origin.id);
            }
        }
        if let Polling::One(poll) = source.poller(None, false)
            && let Ok(Some(polled)) =
                pollster::block_on(poll.polled(&http, None, jiff::Timestamp::UNIX_EPOCH))
        {
            for event in &polled.events {
                let origin = event.origin().expect("a polled issue names its origin");
                issued(event, &origin.id);
            }
        }
    }
}

/// A repository's open pull requests read for a view by every path: each
/// pull request is of its repository, none more than the view lists, and
/// only those past the most read in full read as unknown.
fn queued(bytes: &[u8]) {
    let http = Answering(bytes.to_vec());
    let most = herald_kinds::Most { open: 150, pulls: 2 };
    for forge in FORGES {
        let source = ForgeSource {
            forge,
            options: SourceOptions {
                repository: Some("fuzzer/fuzzed".to_owned()),
                ..SourceOptions::default()
            },
            names: None,
        };
        for token in [None, Some(Credential::Named(Secret::new("TOKEN", "t")))] {
            let Some(queue) = source.queue(token) else {
                continue;
            };
            if let Ok(herald_kinds::Plan::Runs(runs)) =
                pollster::block_on(queue.planned(&http, ("fuzzer/fuzzed", "h", "test"), herald_core::MOST_RUNS))
            {
                assert!(runs.iter().all(|run| run.call.starts_with("POST ")), "{runs:?}");
            }
            if let Ok(counted) = pollster::block_on(queue.capacity(
                &http,
                (herald_kinds::Asked::default(), &["fuzzer/fuzzed"]),
                herald_core::Capacity::Shown,
            )) {
                assert!(
                    counted.capacity.iter().flatten().all(|run| run.repository == "fuzzer/fuzzed"),
                    "{counted:?}"
                );
            }
            let asked = herald_kinds::Asked {
                most,
                files: true,
                ..herald_kinds::Asked::default()
            };
            let Ok(pulls) = pollster::block_on(queue.opened(&http, asked)) else {
                continue;
            };
            assert!(pulls.len() <= most.open, "{}", pulls.len());
            for (index, pull) in pulls.iter().enumerate() {
                assert_eq!(pull.repository, "fuzzer/fuzzed");
                assert_eq!(pull.project, "fuzzed");
                assert!(!pull.unread || index >= most.pulls, "{pull:?}");
            }
        }
    }
    scoped(&http, most);
}

/// What each forge's one query over an owner's repositories makes of the
/// fuzzed answer: only pull requests of the owner's member, named and seen
/// as its membership record says.
fn scoped(http: &Answering, most: herald_kinds::Most) {
    let member = herald_core::Listed {
        id: "7".to_owned(),
        name: "fuzzed".to_owned(),
        path: "fuzzer/fuzzed".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    let members = [member];
    for (forge, owner) in FORGES
        .into_iter()
        .flat_map(|forge| [herald_core::Owner::Organization, herald_core::Owner::User].map(|owner| (forge, owner)))
    {
        let source = ForgeSource {
            forge,
            options: SourceOptions {
                owner: Some("fuzzer".to_owned()),
                ..SourceOptions::default()
            },
            names: None,
        };
        for token in [None, Some(Credential::Named(Secret::new("TOKEN", "t")))] {
            let Some(queue) = source.queue(token) else {
                continue;
            };
            let asked = herald_kinds::Asked {
                most,
                scope: Some(herald_kinds::Scope {
                    owner,
                    members: &members,
                }),
                stated: false,
                files: true,
            };
            let Ok(pulls) = pollster::block_on(queue.opened(http, asked)) else {
                continue;
            };
            assert!(pulls.len() <= most.open, "{}", pulls.len());
            for pull in &pulls {
                assert!(pull.repository.eq_ignore_ascii_case("fuzzer/fuzzed"), "{pull:?}");
                assert_eq!(pull.visibility, herald_core::Visibility::Public, "{pull:?}");
            }
        }
    }
}

/// A bump pattern written from the fuzzed text, and a title read by it and
/// by every shipped reader: a package read is written in the title.
fn bumped(bytes: &[u8]) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let (pattern, title) = text.split_once('\n').unwrap_or((text, text));
    let own: Vec<herald_core::BumpPattern> =
        herald_core::BumpPattern::new(pattern).into_iter().collect();
    if let Some(bump) = herald_core::read_bump(&own, title) {
        assert!(title.contains(&bump.package), "{bump:?}");
    }
}

fuzz_target!(|bytes: &[u8]| {
    queued(bytes);
    bumped(bytes);
    owned(bytes);
    images(bytes);
    runs(bytes);
    alerts(bytes);
    pulls(bytes);
    folded(bytes);
    issues(bytes);
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

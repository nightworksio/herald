use std::sync::atomic::{AtomicUsize, Ordering};

use herald_config::{Config, Source, Uses};
use herald_core::{Listing, Membership, SourceName};
use herald_flows::{Budget, Remembered, Requests, Stop, Sweep, Swept, poll, prepare};
use herald_ports::{Cursors, Http, HttpError, Request, Response};
use herald_sources_forge::{ForgeSource, SourceOptions};
use herald_testing::{FakeClock, FakeSigner};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use crate::harness::*;

mod packages;
mod tags;

pub(crate) const LISTING: &str =
    "/orgs/nightworksio/repos?type=all&sort=full_name&direction=asc&per_page=100&page=1";
const PROBE: &str =
    "/orgs/nightworksio/repos?type=all&sort=full_name&direction=asc&per_page=100&page=2";
const ETAG: (&str, &str) = ("etag", "W/\"1\"");

fn owned(change: impl FnOnce(&mut SourceOptions)) -> Config {
    owned_in("", change)
}

/// The harness config with `extra` on its route, its source made the owner
/// `nightworksio`'s, and changed by `change`.
pub(crate) fn owned_in(extra: &str, change: impl FnOnce(&mut SourceOptions)) -> Config {
    let mut config = config(extra);
    if let Some(source) = config.sources.values_mut().next()
        && let Some(mut forged) = source.of::<ForgeSource>().cloned()
    {
        forged.options.repository = None;
        forged.options.owner = Some("nightworksio".to_owned());
        change(&mut forged.options);
        *source = Source::new(forged);
    }
    config
}

pub(crate) fn repositories(names: &[(u64, &str)]) -> String {
    json!(
        names
            .iter()
            .map(|(id, name)| json!({
                "id": id,
                "name": name,
                "full_name": format!("nightworksio/{name}"),
                "visibility": "public"
            }))
            .collect::<Vec<_>>()
    )
    .to_string()
}

fn releases(name: &str, published: &[(&str, Timestamp)]) -> String {
    json!(
        published
            .iter()
            .map(|(version, at)| json!({
                "tag_name": version,
                "name": format!("{name} {version}"),
                "body": "## Added",
                "draft": false,
                "prerelease": false,
                "published_at": at.to_string(),
                "html_url": format!("https://github.com/nightworksio/{name}/releases/tag/{version}"),
            }))
            .collect::<Vec<_>>()
    )
    .to_string()
}

fn released(name: &str) -> String {
    format!("/repos/nightworksio/{name}/releases?per_page=30")
}

fn described(id: u64, name: &str) -> String {
    format!(r#"{{"id":{id},"name":"{name}"}}"#)
}

/// An `Http` that counts each request on `made` before `script` answers it.
#[derive(Debug, Clone)]
struct Counting<'a> {
    script: Script,
    made: &'a AtomicUsize,
}

impl Http for Counting<'_> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        self.made.fetch_add(1, Ordering::Relaxed);
        self.script.send(request).await
    }
}

fn polled_within(
    config: &Config,
    store: &Failing,
    script: &Script,
    at: Timestamp,
    most: Option<usize>,
) -> Vec<Swept> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let made = AtomicUsize::new(0);
    let counting = Counting {
        script: script.clone(),
        made: &made,
    };
    let connected = prepared.connect(|_| Ok::<_, ()>(counting.clone())).unwrap();
    let clock = FakeClock::at(at);
    let ports = herald_flows::Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget {
            requests: most.map(|most| Requests {
                made: &made,
                from: 0,
                most,
                spare: 0,
            }),
            ..Budget::UNBOUNDED
        },
    };
    pollster::block_on(poll(config, &connected, &ports, "r1"))
}

fn polled(
    config: &Config,
    store: &Failing,
    script: &Script,
    at: Timestamp,
) -> Vec<(Option<String>, Sweep)> {
    polled_within(config, store, script, at, None)
        .into_iter()
        .map(|swept| {
            assert_eq!(swept.source, SourceName::new("gate"));
            (swept.member, swept.outcome)
        })
        .collect()
}

fn listed(members: usize, joined: &[&str], left: &[&str]) -> (Option<String>, Sweep) {
    (
        None,
        Sweep::Listed {
            members,
            joined: joined
                .iter()
                .map(|path| format!("nightworksio/{path}"))
                .collect(),
            left: left
                .iter()
                .map(|path| format!("nightworksio/{path}"))
                .collect(),
        },
    )
}

fn member(path: &str, outcome: Sweep) -> (Option<String>, Sweep) {
    (Some(format!("nightworksio/{path}")), outcome)
}

fn first(found: usize) -> Sweep {
    Sweep::Polled {
        first: true,
        found,
        announced: Vec::new(),
        cursor: Remembered::Advanced,
    }
}

fn members(store: &Failing) -> Vec<Membership> {
    let mut kept: Vec<Membership> =
        pollster::block_on(store.inner.members(&SourceName::new("gate")))
            .unwrap()
            .into_iter()
            .map(|member| member.value)
            .collect();
    kept.sort_by(|one, other| one.repository.id.cmp(&other.repository.id));
    kept
}

fn listing(store: &Failing) -> Option<Listing> {
    pollster::block_on(store.inner.listing(&SourceName::new("gate")))
        .unwrap()
        .map(|record| record.value)
}

/// The first round over `herald` (1) and `gate` (2), each with one release.
fn joined(store: &Failing, config: &Config) {
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let release = releases("herald", &[("v1", NOON)]);
    let gate = releases("gate", &[("v1", NOON)]);
    let script = Script::answering(&[
        (LISTING, 200, &two),
        (&released("herald"), 200, &release),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        (&released("gate"), 200, &gate),
        ("/repos/nightworksio/gate", 200, &described(2, "gate")),
    ])
    .headed(&[ETAG]);
    assert_eq!(
        polled(config, store, &script, NOON),
        [
            listed(2, &["herald", "gate"], &[]),
            member("herald", first(1)),
            member("gate", first(1)),
        ]
    );
}

#[test]
fn an_owner_s_members_join_are_polled_on_their_own_and_a_listing_unchanged_reads_no_page_again() {
    let store = healthy();
    let config = owned(|_| {});
    joined(&store, &config);
    let kept = members(&store);
    assert_eq!(kept.len(), 2);
    assert!(kept.iter().all(|member| member.discovered == NOON
        && member.left.is_none()
        && member.cursor.is_some()));
    let kept = listing(&store).unwrap();
    assert_eq!(
        kept,
        Listing {
            owner: herald_core::Owner::Organization,
            pages: vec![Some("W/\"1\"".to_owned())],
            first: NOON,
            listed: NOON,
            fingerprint: kept.fingerprint.clone(),
            file: None,
            events: None,
        }
    );
    assert_eq!(kept.fingerprint.len(), herald_core::SHA256_HEX_DIGITS);
    let script = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (&released("herald"), 304, ""),
        (&released("gate"), 304, ""),
    ]);
    let later = NOON + SignedDuration::from_hours(1);
    assert_eq!(
        polled(&config, &store, &script, later),
        [
            listed(2, &[], &[]),
            member("herald", Sweep::Unchanged),
            member("gate", Sweep::Unchanged),
        ]
    );
    assert_eq!(listing(&store).unwrap().listed, NOON);
}

#[test]
fn a_member_found_later_announces_only_what_it_publishes_after_and_one_that_leaves_is_not_polled() {
    let store = healthy();
    let config = owned(|_| {});
    joined(&store, &config);
    let found = NOON + SignedDuration::from_hours(1);
    let changed = repositories(&[(2, "gate"), (3, "web")]);
    let web = releases(
        "web",
        &[("v2", found + SignedDuration::from_mins(5)), ("v1", NOON)],
    );
    let (gate_releases, web_releases, web_described) =
        (released("gate"), released("web"), described(3, "web"));
    let mut answers = vec![
        (LISTING, 200, changed.as_str()),
        (LISTING, 200, changed.as_str()),
        (&gate_releases, 304, ""),
    ];
    answers.push((&web_releases, 200, &web));
    answers.push(("/repos/nightworksio/web", 200, &web_described));
    answers.extend([POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let swept = polled(&config, &store, &Script::answering(&answers), found);
    assert_eq!(swept[0], listed(2, &["web"], &["herald"]));
    assert_eq!(swept[1], member("gate", Sweep::Unchanged));
    let (
        path,
        Sweep::Polled {
            first,
            found: count,
            announced,
            cursor,
        },
    ) = &swept[2]
    else {
        panic!("web was not polled: {swept:?}");
    };
    assert_eq!(path.as_deref(), Some("nightworksio/web"));
    assert_eq!((*first, *count, cursor), (true, 2, &Remembered::Advanced));
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].event.version(), Some("v2"));
    assert_eq!(swept.len(), 3);
    let kept = members(&store);
    assert_eq!(kept[0].left, Some(found));
    assert_eq!(kept[2].discovered, found);
    let rejoined = repositories(&[(1, "herald"), (2, "gate"), (3, "web")]);
    let back = found + SignedDuration::from_hours(1);
    let script = Script::answering(&[
        (LISTING, 200, &rejoined),
        (&released("herald"), 304, ""),
        (&released("gate"), 304, ""),
        (&released("web"), 304, ""),
    ]);
    assert_eq!(
        polled(&config, &store, &script, back)[0],
        listed(3, &["herald"], &[])
    );
    let kept = members(&store);
    assert_eq!((kept[0].left, kept[0].discovered), (None, NOON));
}

#[test]
fn a_member_that_left_is_forgotten_with_its_cursor_after_keep_members() {
    let store = healthy();
    let config = owned(|options| {
        options.keep_members = Some(herald_core::Duration(SignedDuration::from_hours(2)));
    });
    joined(&store, &config);
    let one = repositories(&[(2, "gate")]);
    let left = NOON + SignedDuration::from_hours(1);
    let script = Script::answering(&[
        (LISTING, 200, &one),
        (LISTING, 200, &one),
        (&released("gate"), 304, ""),
    ])
    .headed(&[ETAG]);
    assert_eq!(
        polled(&config, &store, &script, left)[0],
        listed(1, &[], &["herald"])
    );
    assert_eq!(members(&store).len(), 2);
    let script = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (&released("gate"), 304, ""),
    ]);
    polled(
        &config,
        &store,
        &script,
        left + SignedDuration::from_hours(2),
    );
    assert_eq!(members(&store).len(), 2);
    let script = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (&released("gate"), 304, ""),
    ]);
    let swept = polled(
        &config,
        &store,
        &script,
        left + SignedDuration::from_hours(3),
    );
    assert_eq!(swept[0], listed(1, &[], &[]));
    assert_eq!(members(&store).len(), 1);
}

#[test]
fn a_listing_that_fails_keeps_the_members_it_knows_and_one_over_a_cap_polls_none() {
    let store = healthy();
    let config = owned(|_| {});
    joined(&store, &config);
    let later = NOON + SignedDuration::from_hours(1);
    let script = Script::answering(&[
        (LISTING, 500, "{}"),
        (&released("herald"), 304, ""),
        (&released("gate"), 304, ""),
    ]);
    let swept = polled(&config, &store, &script, later);
    assert!(matches!(swept[0], (None, Sweep::Stopped(Stop::Source(_)))));
    assert_eq!(swept[1], listed(2, &[], &[]));
    assert_eq!(swept.len(), 4);
    let unreachable = polled(&config, &store, &Script::default(), later);
    assert!(matches!(
        unreachable[0],
        (None, Sweep::Stopped(Stop::Unreachable(_)))
    ));
    let capped = owned(|options| options.most_members = std::num::NonZeroUsize::new(1));
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let script = Script::answering(&[(LISTING, 200, &two), (LISTING, 200, &two)]);
    assert_eq!(
        polled(&capped, &store, &script, later),
        [(
            None,
            Sweep::Stopped(Stop::Source(
                "2 repositories of nightworksio are taken, more than most_members, 1; raise it, or narrow the filters"
                    .to_owned()
            ))
        )]
    );
}

#[test]
fn a_store_that_fails_an_owner_s_records_stops_what_it_cannot_keep() {
    let config = owned(|_| {});
    let reading = Failing {
        fault: Fault::ReadsMembers,
        ..healthy()
    };
    let swept = polled(&config, &reading, &Script::default(), NOON);
    assert!(matches!(
        swept[..],
        [(None, Sweep::Stopped(Stop::Store(_)))]
    ));
    let two = repositories(&[(1, "herald")]);
    let writing = Failing {
        fault: Fault::WritesMembers,
        ..healthy()
    };
    let swept = polled(
        &config,
        &writing,
        &Script::answering(&[(LISTING, 200, &two)]),
        NOON,
    );
    assert!(matches!(swept[0], (None, Sweep::Stopped(Stop::Store(_)))));
    assert_eq!(swept[1], listed(0, &["herald"], &[]));
    assert_eq!(listing(&writing), None);
    let conflicting = Failing {
        fault: Fault::ListingConflicts,
        ..healthy()
    };
    let release = releases("herald", &[("v1", NOON)]);
    let swept = polled(
        &config,
        &conflicting,
        &Script::answering(&[
            (LISTING, 200, &two),
            (&released("herald"), 200, &release),
            ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        ]),
        NOON,
    );
    assert_eq!(
        swept[0],
        (
            None,
            Sweep::Stopped(Stop::Store(
                "another run moved the owner's listing first".to_owned()
            ))
        )
    );
    let moved = Failing {
        fault: Fault::MemberConflicts,
        ..healthy()
    };
    let swept = polled(
        &config,
        &moved,
        &Script::answering(&[
            (LISTING, 200, &two),
            (&released("herald"), 200, &release),
            ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        ]),
        NOON,
    );
    assert_eq!(
        swept[1],
        member(
            "herald",
            Sweep::Polled {
                first: true,
                found: 1,
                announced: Vec::new(),
                cursor: Remembered::Failed(Stop::Store(
                    "another run moved the member's cursor first".to_owned()
                )),
            }
        )
    );
}

#[test]
fn a_member_that_cannot_be_forgotten_is_reported_and_the_listing_kept() {
    let config = owned(|options| {
        options.keep_members = Some(herald_core::Duration(SignedDuration::from_hours(1)));
    });
    let store = Failing {
        fault: Fault::ForgetsMembers,
        ..healthy()
    };
    joined(&store, &config);
    let one = repositories(&[(2, "gate")]);
    let script = Script::answering(&[
        (LISTING, 200, &one),
        (LISTING, 200, &one),
        (&released("gate"), 304, ""),
    ]);
    polled(
        &config,
        &store,
        &script,
        NOON + SignedDuration::from_mins(1),
    );
    let script = Script::answering(&[(LISTING, 200, &one), (&released("gate"), 304, "")]);
    let swept = polled(
        &config,
        &store,
        &script,
        NOON + SignedDuration::from_hours(3),
    );
    assert!(matches!(swept[0], (None, Sweep::Stopped(Stop::Store(_)))));
    assert_eq!(
        listing(&store).unwrap().listed,
        NOON + SignedDuration::from_mins(1)
    );
}

#[test]
fn an_owner_near_its_host_s_request_limit_lists_and_polls_only_what_fits() {
    let store = healthy();
    let config = owned(|_| {});
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let release = releases("herald", &[("v1", NOON)]);
    let script = Script::answering(&[
        (LISTING, 200, &two),
        (&released("herald"), 200, &release),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
    ]);
    let swept: Vec<(Option<String>, Sweep)> =
        polled_within(&config, &store, &script, NOON, Some(3))
            .into_iter()
            .map(|swept| (swept.member, swept.outcome))
            .collect();
    assert_eq!(swept[1], member("herald", first(1)));
    assert!(matches!(
        swept[2],
        (Some(_), Sweep::Stopped(Stop::Limit(_)))
    ));
    let none = polled_within(&config, &healthy(), &Script::default(), NOON, Some(0));
    assert!(matches!(
        none[..],
        [Swept {
            outcome: Sweep::Stopped(Stop::Limit(_)),
            ..
        }]
    ));
}

#[test]
fn a_member_whose_route_stops_keeps_its_cursor_where_it_was() {
    let store = healthy();
    let config = owned(|options| options.backfill = 1);
    let one = repositories(&[(1, "herald")]);
    let release = releases("herald", &[("v1", NOON)]);
    let script = Script::answering(&[
        (LISTING, 200, &one),
        (&released("herald"), 200, &release),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        (
            POSTED.0,
            403,
            r#"{"message":"Missing Access","code":50001}"#,
        ),
    ]);
    let swept = polled(&config, &store, &script, NOON);
    let (
        _,
        Sweep::Polled {
            cursor, announced, ..
        },
    ) = &swept[1]
    else {
        panic!("herald was not polled: {swept:?}");
    };
    assert_eq!(cursor, &Remembered::Kept);
    assert_eq!(announced.len(), 1);
    assert_eq!(members(&store)[0].cursor, None);
}

#[test]
fn a_member_renamed_keeps_its_cursor_and_is_polled_at_its_new_path() {
    let store = healthy();
    let config = owned(|_| {});
    joined(&store, &config);
    let renamed = repositories(&[(1, "herald"), (2, "gatekeeper")]);
    let script = Script::answering(&[
        (LISTING, 200, &renamed),
        (LISTING, 200, &renamed),
        (&released("herald"), 304, ""),
        (&released("gatekeeper"), 304, ""),
    ]);
    let later = NOON + SignedDuration::from_hours(1);
    assert_eq!(
        polled(&config, &store, &script, later)[0],
        listed(2, &[], &[])
    );
    let kept = members(&store);
    assert_eq!(kept[1].repository.path, "nightworksio/gatekeeper");
    assert!(kept[1].cursor.is_some());
}

#[test]
fn a_member_the_selection_no_longer_takes_leaves_though_the_listing_is_unchanged() {
    let store = healthy();
    joined(&store, &owned(|_| {}));
    let narrowed = owned(|options| options.exclude = vec!["gate".to_owned()]);
    let script = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald"), (2, "gate")])),
        (&released("herald"), 304, ""),
    ]);
    let later = NOON + SignedDuration::from_hours(1);
    let swept = polled(&narrowed, &store, &script, later);
    assert_eq!(
        swept,
        [
            listed(1, &[], &["gate"]),
            member("herald", Sweep::Unchanged)
        ]
    );
    assert!(script.seen()[0].ends_with(LISTING));
    let kept = members(&store);
    assert_eq!(kept[1].left, Some(later));
    let failing = Script::answering(&[(LISTING, 500, "{}"), (&released("herald"), 304, "")]);
    let widened = owned(|options| options.exclude = vec!["herald".to_owned()]);
    let swept = polled(
        &widened,
        &store,
        &failing,
        later + SignedDuration::from_hours(1),
    );
    assert!(matches!(swept[0], (None, Sweep::Stopped(Stop::Source(_)))));
    assert_eq!(swept[1], listed(0, &[], &["herald"]));
    assert_eq!(swept.len(), 2);
}

#[test]
fn an_owner_and_a_repository_source_are_polled_side_by_side() {
    let store = healthy();
    let repository = config("").sources[&SourceName::new("gate")].clone();
    let mut both = owned(|_| {});
    both.sources.insert(SourceName::new("mutation"), repository);
    let script = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald")])),
        (&released("herald"), 304, ""),
        (&released("php-mutation-gate"), 304, ""),
    ]);
    let swept = polled_within(&both, &store, &script, NOON, None);
    let outcomes: Vec<(&str, Option<&str>)> = swept
        .iter()
        .map(|swept| (swept.source.as_str(), swept.member.as_deref()))
        .collect();
    assert_eq!(
        outcomes,
        [
            ("gate", None),
            ("gate", Some("nightworksio/herald")),
            ("mutation", None),
        ]
    );
    assert!(matches!(swept[2].outcome, Sweep::Unchanged));
}

#[test]
fn a_github_owner_with_a_token_polls_its_members_in_graphql_batches() {
    let store = healthy();
    let config = owned(|options| {
        options.token = Some(herald_core::SecretRef {
            secret: "HERALD_DISCORD_TOKEN".to_owned(),
        });
        options.graphql_batch = std::num::NonZeroUsize::new(2);
    });
    let graphql = "POST https://api.github.com/graphql";
    let member = |id: u64, name: &str| {
        json!({ "databaseId": id, "releases": { "nodes": [{
            "tagName": "v1",
            "url": format!("https://github.com/nightworksio/{name}/releases/tag/v1"),
            "isDraft": false,
            "name": format!("{name} v1"),
            "description": "## Added",
            "isPrerelease": false,
            "publishedAt": NOON.to_string()
        }] } })
    };
    let two = json!({ "data": { "m0": member(1, "herald"), "m1": member(2, "gate") } }).to_string();
    let one = json!({ "data": { "m0": member(3, "web") } }).to_string();
    let script = Script::answering(&[
        (
            LISTING,
            200,
            &repositories(&[(1, "herald"), (2, "gate"), (3, "web")]),
        ),
        (graphql, 200, &two),
        (graphql, 200, &two),
        (graphql, 200, &one),
        (graphql, 200, &one),
    ]);
    assert_eq!(
        polled(&config, &store, &script, NOON),
        [
            listed(3, &["herald", "gate", "web"], &[]),
            self::member("herald", first(1)),
            self::member("gate", first(1)),
            self::member("web", first(1)),
        ]
    );
    assert_eq!(script.seen().len(), 5);
}

#[test]
fn a_member_whose_template_names_another_source_s_project_from_a_hostile_topic_is_withheld() {
    let store = healthy();
    let repository = config("").sources[&SourceName::new("gate")].clone();
    let mut both = owned(|options| {
        options.project = Some("{{ repository.topics[0] }}".to_owned());
    });
    both.sources.insert(SourceName::new("mutation"), repository);
    both.apps.insert(
        "ci".to_owned(),
        herald_config::App {
            tokens: Vec::new(),
            projects: vec!["deploys".to_owned()],
            routes: Vec::new(),
        },
    );
    let listing = json!([
        { "id": 1, "name": "herald", "full_name": "nightworksio/herald", "visibility": "public", "topics": ["herald"] },
        { "id": 2, "name": "evil", "full_name": "nightworksio/evil", "visibility": "public", "topics": ["PHP-Mutation-Gate"] },
        { "id": 3, "name": "twin", "full_name": "nightworksio/twin", "visibility": "public", "topics": ["shared"] },
        { "id": 4, "name": "copy", "full_name": "nightworksio/copy", "visibility": "public", "topics": ["shared"] },
        { "id": 5, "name": "dep", "full_name": "nightworksio/dep", "visibility": "public", "topics": ["Deploys"] }
    ])
    .to_string();
    let script = Script::answering(&[
        (LISTING, 200, &listing),
        (&released("herald"), 304, ""),
        (&released("php-mutation-gate"), 304, ""),
    ]);
    let swept = polled_within(&both, &store, &script, NOON, None);
    let reasons: Vec<(Option<&str>, String)> = swept
        .iter()
        .filter_map(|swept| match &swept.outcome {
            Sweep::Stopped(Stop::Source(reason)) => Some((swept.member.as_deref(), reason.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        reasons,
        [
            (
                Some("nightworksio/evil"),
                "nightworksio/evil announces as the project PHP-Mutation-Gate, which the source mutation announces as too; its events are withheld until they differ".to_owned()
            ),
            (
                Some("nightworksio/twin"),
                "nightworksio/twin announces as the project shared, which gate's nightworksio/copy announces as too; its events are withheld until they differ".to_owned()
            ),
            (
                Some("nightworksio/copy"),
                "nightworksio/copy announces as the project shared, which gate's nightworksio/twin announces as too; its events are withheld until they differ".to_owned()
            ),
            (
                Some("nightworksio/dep"),
                "nightworksio/dep announces as the project Deploys, which the app ci announces as too; its events are withheld until they differ".to_owned()
            ),
        ]
    );
    let again = Script::answering(&[
        (LISTING, 200, &listing),
        (&released("herald"), 304, ""),
        (&released("php-mutation-gate"), 304, ""),
    ]);
    let later = NOON + SignedDuration::from_hours(1);
    let withheld = polled_within(&both, &store, &again, later, None)
        .iter()
        .filter(|swept| matches!(swept.outcome, Sweep::Stopped(Stop::Source(_))))
        .count();
    assert_eq!(withheld, 4);
    let evil = members(&store)
        .into_iter()
        .find(|member| member.repository.id == "2")
        .unwrap();
    assert!(evil.collides.is_some());
    assert!(members(&store)[0].collides.is_none());
}

/// Each member the round withheld, by its path, with why.
fn withheld(swept: &[Swept]) -> Vec<(String, String)> {
    swept
        .iter()
        .filter_map(|swept| match (&swept.member, &swept.outcome) {
            (Some(member), Sweep::Stopped(Stop::Source(reason))) => Some((
                format!("{}:{member}", swept.source.as_str()),
                reason.clone(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn two_owners_joining_the_same_repositories_in_one_round_withhold_every_one_untemplated() {
    let store = healthy();
    let mut twice = owned(|_| {});
    let gate = twice.sources[&SourceName::new("gate")].clone();
    twice.sources.insert(SourceName::new("other"), gate);
    let one = repositories(&[(1, "herald")]);
    let script = Script::answering(&[(LISTING, 200, &one), (LISTING, 200, &one)]);
    assert_eq!(
        withheld(&polled_within(&twice, &store, &script, NOON, None)),
        [
            (
                "gate:nightworksio/herald".to_owned(),
                "nightworksio/herald announces as the project herald, which other's nightworksio/herald announces as too; its events are withheld until they differ".to_owned()
            ),
            (
                "other:nightworksio/herald".to_owned(),
                "nightworksio/herald announces as the project herald, which gate's nightworksio/herald announces as too; its events are withheld until they differ".to_owned()
            ),
        ]
    );
}

#[test]
fn a_member_announcing_as_the_project_a_feed_last_announced_is_withheld() {
    let store = healthy();
    let mut config = owned(|_| {});
    config.sources.insert(
        SourceName::new("news"),
        herald_config::Source::new(herald_sources_feed::FeedOptions {
            url: "https://news.example/feed.xml".to_owned(),
            ..herald_sources_feed::FeedOptions::default()
        }),
    );
    pollster::block_on(store.inner.put_cursor(
        &SourceName::new("news"),
        &herald_core::Cursor {
            project: Some("Herald".to_owned()),
            ..herald_core::Cursor::default()
        },
        None,
    ))
    .unwrap();
    let script = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald")])),
        ("/feed.xml", 304, ""),
    ]);
    assert_eq!(
        withheld(&polled_within(&config, &store, &script, NOON, None)),
        [(
            "gate:nightworksio/herald".to_owned(),
            "nightworksio/herald announces as the project herald, which the source news announces as too; its events are withheld until they differ".to_owned()
        )]
    );
}

#[test]
fn a_source_whose_records_cannot_be_read_withholds_every_member_that_round() {
    let mut store = healthy();
    let mut config = owned(|_| {});
    let repository = crate::harness::config("").sources[&SourceName::new("gate")].clone();
    config
        .sources
        .insert(SourceName::new("mutation"), repository);
    store.fault = Fault::Cursors;
    let script = Script::answering(&[(LISTING, 200, &repositories(&[(1, "herald")]))]);
    let swept = polled_within(&config, &store, &script, NOON, None);
    assert_eq!(
        withheld(&swept),
        [(
            "gate:nightworksio/herald".to_owned(),
            "the records of mutation could not be read, so whether another announces as its project herald cannot be checked; its events are withheld this round".to_owned()
        )]
    );
    assert!(matches!(
        swept.last().map(|swept| &swept.outcome),
        Some(Sweep::Stopped(Stop::Store(_)))
    ));
}

#[test]
fn an_owner_following_its_events_polls_only_the_members_they_name() {
    const EVENTS: &str = "/orgs/nightworksio/events?per_page=100&page=1";
    let store = healthy();
    let config = owned(|options| {
        options.transport = Some(herald_sources_forge::Transport::Events);
    });
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let release = releases("herald", &[("v1", NOON)]);
    let gate = releases("gate", &[("v1", NOON)]);
    let opening = Script::answering(&[
        (LISTING, 200, &two),
        (
            EVENTS,
            200,
            r#"[{"id":"10","type":"PushEvent","repo":{"id":1}}]"#,
        ),
        (&released("herald"), 200, &release),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        (&released("gate"), 200, &gate),
        ("/repos/nightworksio/gate", 200, &described(2, "gate")),
    ])
    .headed(&[ETAG]);
    assert_eq!(
        polled(&config, &store, &opening, NOON),
        [
            listed(2, &["herald", "gate"], &[]),
            member("herald", first(1)),
            member("gate", first(1)),
        ]
    );
    assert_eq!(
        listing(&store).unwrap().events,
        Some(herald_core::EventsRead {
            etag: Some("W/\"1\"".to_owned()),
            newest: 10
        })
    );
    let later = NOON + SignedDuration::from_hours(1);
    let quiet = Script::answering(&[(LISTING, 304, ""), (PROBE, 200, "[]"), (EVENTS, 304, "")]);
    assert_eq!(
        polled(&config, &store, &quiet, later),
        [
            listed(2, &[], &[]),
            member("herald", Sweep::Unchanged),
            member("gate", Sweep::Unchanged),
        ]
    );
    let named = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (
            EVENTS,
            200,
            r#"[{"id":"12","type":"ReleaseEvent","repo":{"id":1},"payload":{"action":"published"}},{"id":"10","type":"PushEvent","repo":{"id":1}}]"#,
        ),
        (&released("herald"), 304, ""),
    ]);
    assert_eq!(
        polled(
            &config,
            &store,
            &named,
            later + SignedDuration::from_hours(1)
        ),
        [
            listed(2, &[], &[]),
            member("gate", Sweep::Unchanged),
            member("herald", Sweep::Unchanged),
        ]
    );
    assert_eq!(listing(&store).unwrap().events.unwrap().newest, 12);
    let failing = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (EVENTS, 500, "{}"),
        (&released("herald"), 304, ""),
        (&released("gate"), 304, ""),
    ]);
    let swept = polled(
        &config,
        &store,
        &failing,
        later + SignedDuration::from_hours(2),
    );
    assert!(matches!(swept[0], (None, Sweep::Stopped(Stop::Source(_)))));
    assert_eq!(swept.len(), 4);
}

#[test]
fn an_owner_following_its_events_reads_none_where_the_host_s_request_limit_is_spent() {
    let store = healthy();
    let config = owned(|options| {
        options.transport = Some(herald_sources_forge::Transport::Events);
    });
    let script = Script::answering(&[(LISTING, 200, &repositories(&[(1, "herald")]))]);
    let swept = polled_within(&config, &store, &script, NOON, Some(1));
    assert_eq!(script.seen().len(), 1);
    assert!(
        swept
            .iter()
            .any(|swept| matches!(swept.outcome, Sweep::Stopped(Stop::Limit(_)))),
        "{swept:?}"
    );
}

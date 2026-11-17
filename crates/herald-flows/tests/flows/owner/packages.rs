use std::sync::Arc;

use jiff::SignedDuration;
use serde_json::json;

use super::*;

pub(super) const PACKAGES: &str =
    "/orgs/nightworksio/packages?package_type=container&per_page=100&page=1";
pub(super) const VERSIONS: &str =
    "/orgs/nightworksio/packages/container/gate-image/versions?per_page=30";
const EVENTS: &str = "/orgs/nightworksio/events?per_page=100&page=1";

#[test]
fn an_owner_following_its_events_polls_each_member_its_package_listing_names() {
    let store = healthy();
    let config = owned(|options| {
        options.transport = Some(herald_sources_forge::Transport::Events);
        options.signals = Some(vec![
            herald_core::Signal::Release,
            herald_core::Signal::Package,
        ]);
        options.package_types = Some(vec!["container".to_owned()]);
    });
    let listed_packages = json!([{
        "name": "gate-image",
        "html_url": "https://github.com/orgs/nightworksio/packages/container/package/gate-image",
        "repository": {"id": 2}
    }])
    .to_string();
    let versions = json!([{
        "name": "sha256:abc",
        "created_at": NOON.to_string(),
        "metadata": {"container": {"tags": ["v1"]}}
    }])
    .to_string();
    let opening = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald"), (2, "gate")])),
        (
            EVENTS,
            200,
            r#"[{"id":"10","type":"PushEvent","repo":{"id":1}}]"#,
        ),
        (PACKAGES, 200, &listed_packages),
        (
            &released("herald"),
            200,
            &releases("herald", &[("v1", NOON)]),
        ),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        (&released("gate"), 200, &releases("gate", &[("v1", NOON)])),
        ("/repos/nightworksio/gate", 200, &described(2, "gate")),
        (VERSIONS, 200, &versions),
    ])
    .headed(&[ETAG]);
    assert_eq!(
        polled(&config, &store, &opening, NOON),
        [
            listed(2, &["herald", "gate"], &[]),
            member("herald", first(1)),
            member("gate", first(2)),
        ]
    );
    let later = NOON + SignedDuration::from_hours(1);
    let quiet = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (EVENTS, 304, ""),
        (PACKAGES, 200, &listed_packages),
        (&released("gate"), 304, ""),
        ("/repos/nightworksio/gate", 200, &described(2, "gate")),
        (VERSIONS, 304, ""),
    ]);
    assert_eq!(
        polled(&config, &store, &quiet, later),
        [
            listed(2, &[], &[]),
            member("herald", Sweep::Unchanged),
            member("gate", Sweep::Unchanged),
        ]
    );
    assert!(
        quiet.seen().iter().any(|seen| seen.ends_with(VERSIONS)),
        "the named member's image is asked"
    );
    let failing = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (EVENTS, 304, ""),
        (PACKAGES, 500, "{}"),
    ]);
    assert_eq!(
        polled(
            &config,
            &store,
            &failing,
            later + SignedDuration::from_hours(1)
        ),
        [
            (
                None,
                Sweep::Stopped(Stop::Source(
                    "GitHub's packages was answered with 500".to_owned()
                ))
            ),
            listed(2, &[], &[]),
            member("herald", Sweep::Unchanged),
            member("gate", Sweep::Unchanged),
        ]
    );
}

/// The harness config with its one repository source made a `forge`
/// source announcing packages, and changed by `change`.
fn packaged(forge: herald_sources_forge::Forge, change: impl FnOnce(&mut SourceOptions)) -> Config {
    let mut config = config("");
    if let Some(source) = config.sources.values_mut().next()
        && let Some(mut forged) = source.of::<ForgeSource>().cloned()
    {
        forged.forge = forge;
        forged.options.signals = Some(vec![herald_core::Signal::Package]);
        change(&mut forged.options);
        *source = Source::new(forged);
    }
    config
}

#[test]
fn a_user_s_repository_lists_its_owner_s_packages_and_remembers_what_is_past_its_page() {
    let store = healthy();
    let config = packaged(herald_sources_forge::Forge::GitHub, |options| {
        options.package_types = Some(vec!["container".to_owned()]);
        options.page = std::num::NonZeroUsize::new(1);
    });
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "html_url": "https://github.com/nightworksio/php-mutation-gate",
        "owner": {"login": "nightworksio", "type": "User"}
    })
    .to_string();
    let linked = json!([{
        "name": "gate",
        "html_url": "https://github.com/orgs/nightworksio/packages/container/package/gate",
        "repository": {"id": 5}
    }])
    .to_string();
    let tagged = |tag: &str, at: Timestamp| {
        json!({
            "name": format!("sha256:{tag}"),
            "created_at": at.to_string(),
            "metadata": {"container": {"tags": [tag]}}
        })
    };
    let versions = json!([
        tagged("v2", NOON),
        tagged("v1", NOON - SignedDuration::from_hours(1))
    ])
    .to_string();
    let script = Script::answering(&[
        ("/repos/nightworksio/php-mutation-gate", 200, &described),
        (
            "/users/nightworksio/packages?package_type=container&per_page=100&page=1",
            200,
            &linked,
        ),
        (
            "/users/nightworksio/packages/container/gate/versions?per_page=1",
            200,
            &versions,
        ),
    ]);
    assert_eq!(polled(&config, &store, &script, NOON), [(None, first(1))]);
}

#[test]
fn a_gitlab_project_lists_its_own_packages_of_one_type_or_several() {
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "web_url": "https://gitlab.com/nightworksio/php-mutation-gate",
        "path_with_namespace": "nightworksio/php-mutation-gate"
    })
    .to_string();
    let listed = json!([{
        "name": "gate",
        "version": "1.0.0",
        "package_type": "npm",
        "created_at": NOON.to_string()
    }])
    .to_string();
    for (types, asked) in [
        (vec!["npm"], "&package_type=npm"),
        (vec!["npm", "pypi"], ""),
    ] {
        let store = healthy();
        let config = packaged(herald_sources_forge::Forge::GitLab, |options| {
            options.package_types = Some(types.iter().map(|kind| (*kind).to_owned()).collect());
        });
        let listing = format!(
            "/projects/nightworksio%2Fphp-mutation-gate/packages?order_by=created_at&sort=desc&per_page=30{asked}"
        );
        let script = Script::answering(&[
            (
                "/projects/nightworksio%2Fphp-mutation-gate",
                200,
                &described,
            ),
            (&listing, 200, &listed),
        ]);
        assert_eq!(polled(&config, &store, &script, NOON), [(None, first(1))]);
    }
}

#[test]
fn an_organization_s_repository_lists_its_packages_under_the_organization() {
    let store = healthy();
    let config = packaged(herald_sources_forge::Forge::GitHub, |options| {
        options.package_types = Some(vec!["container".to_owned()]);
    });
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "html_url": "https://github.com/nightworksio/php-mutation-gate",
        "owner": {"login": "nightworksio", "type": "Organization"}
    })
    .to_string();
    let script = Script::answering(&[
        ("/repos/nightworksio/php-mutation-gate", 200, &described),
        (PACKAGES, 200, "[]"),
    ]);
    assert_eq!(polled(&config, &store, &script, NOON), [(None, first(0))]);
}

#[test]
fn a_member_whose_owner_s_package_listing_failed_announces_its_releases() {
    let store = healthy();
    let config = owned(|options| {
        options.signals = Some(vec![
            herald_core::Signal::Release,
            herald_core::Signal::Package,
        ]);
        options.package_types = Some(vec!["container".to_owned()]);
    });
    let script = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald")])),
        (PACKAGES, 500, "{}"),
        (
            &released("herald"),
            200,
            &releases("herald", &[("v1", NOON)]),
        ),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
    ]);
    assert_eq!(
        polled(&config, &store, &script, NOON),
        [
            (
                None,
                Sweep::Stopped(Stop::Source(
                    "GitHub's packages was answered with 500".to_owned()
                ))
            ),
            listed(1, &["herald"], &[]),
            member("herald", first(1)),
        ]
    );
}

#[test]
fn a_member_polled_through_graphql_whose_releases_and_packages_are_unchanged_is_unchanged() {
    let store = healthy();
    let config = owned(|options| {
        options.token = Some(herald_core::SecretRef {
            secret: "HERALD_DISCORD_TOKEN".to_owned(),
        });
        options.signals = Some(vec![
            herald_core::Signal::Release,
            herald_core::Signal::Package,
        ]);
        options.package_types = Some(vec!["container".to_owned()]);
    });
    let graphql = "POST https://api.github.com/graphql";
    let answered = json!({ "data": { "m0": { "databaseId": 1, "releases": { "nodes": [{
        "tagName": "v1",
        "url": "https://github.com/nightworksio/herald/releases/tag/v1",
        "isDraft": false,
        "name": "herald v1",
        "description": "## Added",
        "isPrerelease": false,
        "publishedAt": NOON.to_string()
    }] } } } })
    .to_string();
    let opening = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald")])),
        (PACKAGES, 200, "[]"),
        (graphql, 200, &answered),
        (graphql, 200, &answered),
    ])
    .headed(&[ETAG]);
    assert_eq!(
        polled(&config, &store, &opening, NOON),
        [listed(1, &["herald"], &[]), member("herald", first(1))]
    );
    let quiet = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (PACKAGES, 200, "[]"),
        (graphql, 200, &answered),
    ]);
    assert_eq!(
        polled(
            &config,
            &store,
            &quiet,
            NOON + SignedDuration::from_hours(1)
        ),
        [listed(1, &[], &[]), member("herald", Sweep::Unchanged)]
    );
}

fn image(name: &str, visibility: Option<&str>, repository: u64) -> serde_json::Value {
    json!({
        "name": name,
        "html_url": format!("https://github.com/orgs/nightworksio/packages/container/package/{name}"),
        "repository": {"id": repository},
        "visibility": visibility
    })
}

pub(super) fn tagged(tag: &str) -> String {
    json!([{
        "name": "sha256:abc",
        "created_at": NOON.to_string(),
        "metadata": {"container": {"tags": [tag]}}
    }])
    .to_string()
}

fn posts(script: &Script) -> usize {
    script
        .seen()
        .iter()
        .filter(|seen| seen.starts_with("POST") && seen.ends_with("/messages"))
        .count()
}

#[test]
fn a_package_not_everyone_may_see_reaches_no_public_route_though_its_repository_is_public() {
    let store = healthy();
    let config = packaged(herald_sources_forge::Forge::GitHub, |options| {
        options.package_types = Some(vec!["container".to_owned()]);
        options.backfill = 3;
    });
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "html_url": "https://github.com/nightworksio/php-mutation-gate",
        "visibility": "public",
        "owner": {"login": "nightworksio", "type": "Organization"}
    })
    .to_string();
    let listed = json!([
        image("open", Some("public"), 5),
        image("closed", Some("private"), 5),
        image("staff", Some("internal"), 5),
        image("unnamed", None, 5)
    ])
    .to_string();
    let versions =
        |name: &str| format!("/orgs/nightworksio/packages/container/{name}/versions?per_page=30");
    let (open, closed, staff, unnamed) = (
        versions("open"),
        versions("closed"),
        versions("staff"),
        versions("unnamed"),
    );
    let mut answers = vec![
        (
            "/repos/nightworksio/php-mutation-gate",
            200,
            described.as_str(),
        ),
        (PACKAGES, 200, listed.as_str()),
    ];
    let tags = [tagged("v1"), tagged("v2"), tagged("v3"), tagged("v4")];
    for (path, body) in [&open, &closed, &staff, &unnamed].into_iter().zip(&tags) {
        answers.push((path.as_str(), 200, body.as_str()));
    }
    answers.extend([POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let script = Script::answering(&answers);
    let swept = polled(&config, &store, &script, NOON);
    assert_eq!(
        posts(&script),
        1,
        "only the public image is posted: {swept:?}"
    );
}

/// How many posts a private member's public image makes, the source
/// seeing packages as `seen` says.
fn private_member_posts(seen: Option<herald_sources_forge::PackageVisibility>) -> usize {
    let store = healthy();
    let config = owned(|options| {
        options.signals = Some(vec![
            herald_core::Signal::Release,
            herald_core::Signal::Package,
        ]);
        options.package_types = Some(vec!["container".to_owned()]);
        options.visibility = Some(vec![
            herald_core::Visibility::Public,
            herald_core::Visibility::Private,
        ]);
        options.backfill = 2;
        options.package_visibility = seen;
    });
    let private = json!([{
        "id": 2,
        "name": "gate",
        "full_name": "nightworksio/gate",
        "visibility": "private"
    }])
    .to_string();
    let listed = json!([image("gate-image", Some("public"), 2)]).to_string();
    let versions = tagged("v1");
    let release = releases("gate", &[("v0", NOON - SignedDuration::from_hours(1))]);
    let (repository, asked) = (described(2, "gate"), released("gate"));
    let mut answers = vec![
        (LISTING, 200, private.as_str()),
        (PACKAGES, 200, listed.as_str()),
        (asked.as_str(), 200, release.as_str()),
        ("/repos/nightworksio/gate", 200, repository.as_str()),
        (VERSIONS, 200, versions.as_str()),
    ];
    answers.extend([POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let script = Script::answering(&answers);
    polled(&config, &store, &script, NOON);
    posts(&script)
}

#[test]
fn a_public_package_of_a_private_member_reaches_no_public_route_by_default() {
    assert_eq!(private_member_posts(None), 0);
    assert_eq!(
        private_member_posts(Some(herald_sources_forge::PackageVisibility::Stricter)),
        0
    );
}

#[test]
fn a_public_package_of_a_private_member_reaches_a_public_route_where_the_package_decides() {
    assert_eq!(
        private_member_posts(Some(herald_sources_forge::PackageVisibility::Package)),
        1,
        "the public image is posted and the private release is not"
    );
}

#[test]
fn a_version_whose_page_is_no_web_address_stops_its_source() {
    let store = healthy();
    let config = packaged(herald_sources_forge::Forge::GitHub, |options| {
        options.package_types = Some(vec!["container".to_owned()]);
    });
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "html_url": "https://github.com/nightworksio/php-mutation-gate",
        "owner": {"login": "nightworksio", "type": "Organization"}
    })
    .to_string();
    let listed = json!([image("gate", Some("public"), 5)]).to_string();
    let versions = json!([{
        "name": "sha256:abc",
        "html_url": "not a page",
        "created_at": NOON.to_string(),
        "metadata": {"container": {"tags": ["v1"]}}
    }])
    .to_string();
    let script = Script::answering(&[
        ("/repos/nightworksio/php-mutation-gate", 200, &described),
        (PACKAGES, 200, &listed),
        (
            "/orgs/nightworksio/packages/container/gate/versions?per_page=30",
            200,
            &versions,
        ),
    ]);
    let swept = polled(&config, &store, &script, NOON);
    assert!(
        matches!(&swept[..], [(None, Sweep::Stopped(Stop::Source(_)))]),
        "{swept:?}"
    );
}

/// A repository source taking its packages and a ghcr source of its owner,
/// both on one route, polled one after the other.
pub(super) const DEDUPED: &str = r#"version = 1
[store]
kind = "sqlite"
[run]
sources = 1
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
token = { secret = "GITHUB_TOKEN" }
signals = ["package"]
package_types = ["container"]
backfill = 1
[sources.images]
kind = "ghcr"
owner = "nightworksio"
token = { secret = "GITHUB_TOKEN" }
backfill = 1
[[routes]]
id = "releases"
sources = ["gate", "images"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
[templates.release]
title = "{{ event.project }} {{ event.attributes.version }}"
body = "{{ event.body }}"
"#;

#[test]
fn an_image_a_repository_s_package_signal_and_a_ghcr_source_both_see_is_announced_once() {
    let store = healthy();
    let config = herald_config::load(
        "herald.toml",
        DEDUPED,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "html_url": "https://github.com/nightworksio/php-mutation-gate",
        "visibility": "public",
        "private": false,
        "owner": {"login": "nightworksio", "type": "Organization"}
    })
    .to_string();
    let listing = json!([{
        "id": 77,
        "name": "gate-image",
        "html_url": "https://github.com/orgs/nightworksio/packages/container/package/gate-image",
        "visibility": "public",
        "repository": {"id": 5, "name": "php-mutation-gate", "full_name": "nightworksio/php-mutation-gate", "visibility": "public"}
    }])
    .to_string();
    let versions = tagged("v1");
    let mut answers = vec![
        (PACKAGES, 200, listing.as_str()),
        (
            "/repos/nightworksio/php-mutation-gate",
            200,
            described.as_str(),
        ),
        (VERSIONS, 200, versions.as_str()),
    ];
    answers.extend(crate::poll::announced());
    let script = Keyed {
        answers: answers
            .into_iter()
            .map(|(line, status, body)| (line.to_owned(), status, body.to_owned()))
            .collect(),
        seen: Arc::default(),
    };
    let tokened = secrets().with("GITHUB_TOKEN", "github-token");
    let prepared = pollster::block_on(herald_flows::prepare(
        &config,
        &tokened,
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = herald_flows::Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let swept = pollster::block_on(poll(&config, &connected, &ports, "r1"));
    let seen = script.seen.lock().unwrap().clone();
    let posted = seen
        .iter()
        .filter(|line| line.starts_with("POST") && line.ends_with("/messages"))
        .count();
    assert_eq!(posted, 1, "{seen:?} {swept:?}");
    assert!(
        swept
            .iter()
            .all(|swept| !matches!(swept.outcome, herald_flows::Sweep::Stopped(_))),
        "{swept:?}"
    );
    assert!(
        swept
            .iter()
            .any(|swept| swept.source == SourceName::new("images"))
    );
}

/// Answers each request with the first answer whose line ends its own,
/// whatever order the requests come in, and keeps each request's line.
#[derive(Clone)]
pub(super) struct Keyed {
    pub(super) answers: Vec<(String, u16, String)>,
    pub(super) seen: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Http for Keyed {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let line = format!("{} {}", request.method.as_str(), request.url.redacted());
        self.seen.lock().unwrap().push(line.clone());
        self.answers
            .iter()
            .find(|(expected, _, _)| line.ends_with(expected.as_str()))
            .map(|(_, status, body)| Response {
                status: *status,
                headers: Vec::new(),
                body: body.clone().into_bytes(),
            })
            .ok_or(HttpError::Unreachable { reason: line })
    }
}

#[test]
fn a_ghcr_image_announcing_as_an_unrelated_source_s_project_is_withheld() {
    let store = healthy();
    let config = herald_config::load(
        "herald.toml",
        &DEDUPED.replace(
            "backfill = 1\n[[routes]]",
            "backfill = 1\nproject = \"php-mutation-gate\"\n[[routes]]",
        ),
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "visibility": "public",
        "private": false,
        "owner": {"login": "nightworksio", "type": "Organization"}
    })
    .to_string();
    let listing = json!([{
        "id": 78,
        "name": "other-image",
        "visibility": "public",
        "repository": {"id": 6, "name": "other", "full_name": "nightworksio/other", "visibility": "public"}
    }])
    .to_string();
    let script = Keyed {
        answers: vec![
            (PACKAGES.to_owned(), 200, listing),
            (
                "/repos/nightworksio/php-mutation-gate".to_owned(),
                200,
                described,
            ),
        ],
        seen: Arc::default(),
    };
    let tokened = secrets().with("GITHUB_TOKEN", "github-token");
    let prepared = pollster::block_on(herald_flows::prepare(
        &config,
        &tokened,
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = herald_flows::Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let swept = pollster::block_on(poll(&config, &connected, &ports, "r1"));
    assert!(
        swept.iter().any(|swept| swept.member.as_deref() == Some("other-image")
            && swept.outcome
                == herald_flows::Sweep::Stopped(herald_flows::Stop::Source(
                    "other-image announces as the project php-mutation-gate, which the source gate announces as too; its events are withheld until they differ".to_owned()
                ))),
        "{swept:?}"
    );
}

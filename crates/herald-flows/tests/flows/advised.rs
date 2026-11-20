//! A view's advisories kept to the repositories it holds, and a living
//! post's section recalled once its route no longer receives one
//! (ADR-0067).

use std::collections::BTreeMap;

use herald_config::{Config, Uses, load};
use herald_core::{Board, Listed, Listing, Membership, Owner, SourceName, Visibility};
use herald_flows::{Budget, Connected, Outcome, Ports, Sweep, Swept, prepare, recalls};
use herald_ports::{Cursors, Response, Store};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

use crate::advisories::{
    ALERT_PAGE, CHANNEL_READ, CODE, DEPENDABOT, EDITED, GUILD_READ, ONBOARDING_READ, SECRETS,
    WITHHELD, alerts, answers, board, page, posted, reported, round, routed,
};
use crate::harness::{Failing, Fault, NOON, POSTED, Script, healthy};

const SEARCH: &str =
    "/search/issues?q=is%3Apr%20is%3Aopen%20org%3Ao&sort=updated&order=desc&per_page=100&page=1";
const GRAPHQL: &str = "https://api.github.com/graphql";
const RECALLED: &str = "removed because this route no longer receives it";

fn owned(view: &str) -> Config {
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[sources.org]\nkind = \"github\"\nowner = \"o\"\ntoken = {{ secret = \"HERALD_GITHUB_TOKEN\" }}\nvisibility = [\"public\", \"private\"]\n[views.security]\nsources = [\"org\"]\noverviews = [\"advisories\"]\nrepositories = [\"o/pay*\"]\n{view}\n"
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn member(id: &str, name: &str) -> Listed {
    Listed {
        id: id.to_owned(),
        name: name.to_owned(),
        path: format!("o/{name}"),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Private,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

/// A store holding the organization `o`'s listing, `pay-api` and `other`
/// its members.
fn listed() -> Failing {
    listed_of(&["pay-api", "other"])
}

/// A store holding the organization `o`'s listing, `names` its members.
fn listed_of(names: &[&str]) -> Failing {
    let store = healthy();
    let source = SourceName::new("org");
    let listing = Listing {
        owner: Owner::Organization,
        pages: Vec::new(),
        first: NOON,
        listed: NOON,
        fingerprint: String::new(),
        file: None,
        events: None,
        stale: false,
    };
    pollster::block_on(store.put_listing(&source, &listing, None)).unwrap();
    for (id, name) in (1..).zip(names) {
        let repository = member(&id.to_string(), name);
        let membership = Membership {
            repository,
            discovered: NOON,
            left: None,
            cursor: None,
            collides: None,
            hooked: false,
        };
        pollster::block_on(store.put_member(&source, &membership, None)).unwrap();
    }
    store
}

fn found(number: u64, repository: &str) -> Value {
    json!({ "number": number, "title": "t",
        "html_url": format!("https://github.com/o/{repository}/pull/{number}"),
        "created_at": "2026-10-30T10:00:00Z", "updated_at": "2026-10-30T11:00:00Z",
        "repository_url": format!("https://api.github.com/repos/o/{repository}") })
}

fn node(number: u64, repository: &str) -> Value {
    json!({ "number": number, "title": format!("read {number}"),
        "url": format!("https://github.com/o/{repository}/pull/{number}"), "isDraft": false,
        "createdAt": "2026-10-30T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z",
        "headRefOid": format!("h{number}") })
}

/// The search finding a pull request in each of `pay-api` and `other`,
/// then GraphQL reading both.
fn searched() -> Vec<(String, u16, String)> {
    let search = json!({ "total_count": 2, "items": [found(1, "pay-api"), found(2, "other")] });
    let read = json!({ "data": {
        "r0": { "visibility": "PRIVATE", "p0": node(1, "pay-api") },
        "r1": { "visibility": "PRIVATE", "p0": node(2, "other") }
    } });
    vec![
        (SEARCH.to_owned(), 200, search.to_string()),
        (GRAPHQL.to_owned(), 200, read.to_string()),
    ]
}

fn alert(number: u64, repository: &str) -> Value {
    json!({
        "number": number, "state": "open",
        "html_url": format!("https://github.com/{repository}/security/dependabot/{number}"),
        "created_at": "2026-10-01T10:00:00Z",
        "security_advisory": { "summary": "Prototype pollution", "severity": "critical" },
        "repository": { "full_name": repository },
    })
}

/// The board `herald queue --private` reads of the view `security`.
fn queued(config: &Config, store: &Failing, answers: &[(String, u16, String)]) -> (Board, Script) {
    let answers: Vec<(&str, u16, &str)> = answers
        .iter()
        .map(|(line, status, body)| (line.as_str(), *status, body.as_str()))
        .collect();
    let script = Script::answering(&answers);
    let secrets = FakeSecrets::default().with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let board = pollster::block_on(herald_flows::queue(
        config,
        &connected,
        &ports,
        ("security", true),
    ))
    .unwrap();
    (board, script)
}

fn answer(body: String, next: Option<String>) -> Response {
    Response {
        status: 200,
        headers: next
            .map(|next| vec![("link".to_owned(), format!("<{next}>; rel=\"next\""))])
            .unwrap_or_default(),
        body: body.into_bytes(),
    }
}

/// The forge of `searched`, `o/pay-api` holding one open alert, and the
/// organization's own listings holding `hidden` alerts of `o/other`.
fn forge(hidden: u64) -> Script {
    let api = "https://api.github.com";
    let mut held = BTreeMap::new();
    for (line, _, body) in searched() {
        let url = if line.starts_with("https://") {
            line
        } else {
            format!("{api}{line}")
        };
        held.insert(url, answer(body, None));
    }
    for segment in ["code-scanning", "dependabot", "secret-scanning"] {
        let hidden_secret = if segment == "secret-scanning" {
            "&hide_secret=true"
        } else {
            ""
        };
        let first = format!("/{segment}/alerts?state=open&per_page=100{hidden_secret}");
        let own = if segment == "dependabot" {
            json!([alert(1, "o/pay-api")])
        } else {
            json!([])
        };
        held.insert(
            format!("{api}/repos/o/pay-api{first}"),
            answer(own.to_string(), None),
        );
        let pages = hidden.div_ceil(100).max(1);
        for page in 1..=pages {
            let url = match page {
                1 => format!("{api}/orgs/o{first}"),
                _ => format!("{api}/orgs/o{first}&page={page}"),
            };
            let alerts: Vec<Value> = ((page - 1) * 100..(page * 100).min(hidden))
                .map(|number| alert(number + 2, "o/other"))
                .collect();
            let next = (page < pages).then(|| format!("{api}/orgs/o{first}&page={}", page + 1));
            held.insert(url, answer(json!(alerts).to_string(), next));
        }
    }
    Script::keyed(held)
}

/// The board `herald queue --private` reads of the view `security` from
/// `forge`.
fn read_from(config: &Config, forge: &Script) -> Board {
    read_with(config, forge, &listed())
}

/// The board `herald queue --private` reads of the view `security` from
/// `forge`, `store` holding the owner's members.
fn read_with(config: &Config, forge: &Script, store: &Failing) -> Board {
    let secrets = FakeSecrets::default().with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(forge.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    pollster::block_on(herald_flows::queue(
        config,
        &connected,
        &ports,
        ("security", true),
    ))
    .unwrap()
}

#[test]
fn a_view_that_holds_some_repositories_shows_the_same_whatever_those_it_hides_hold() {
    let config = owned("");
    let none = read_from(&config, &forge(0));
    let many = read_from(&config, &forge(5_000));
    assert_eq!(format!("{none:?}"), format!("{many:?}"));
    let shown = none.overviews.advisories.unwrap();
    let found = shown.shown().unwrap();
    let repositories: Vec<&str> = found
        .open
        .iter()
        .map(|row| row.repository.as_str())
        .collect();
    assert_eq!(repositories, ["o/pay-api"]);
    assert_eq!((found.unread.len(), found.more), (0, false));
}

/// The forge of `searched` whose owner lists `pushed` as its newest
/// pushed two, each member of `pay` holding no open alert but `pay-api`
/// one.
fn pushed_first(pushed: &[&str]) -> Script {
    let api = "https://api.github.com";
    let mut held = BTreeMap::new();
    for (line, _, body) in searched() {
        let url = if line.starts_with("https://") {
            line
        } else {
            format!("{api}{line}")
        };
        held.insert(url, answer(body, None));
    }
    let listed: Vec<Value> = pushed
        .iter()
        .map(|name| json!({ "full_name": format!("o/{name}") }))
        .collect();
    held.insert(
        format!("{api}/orgs/o/repos?sort=pushed&direction=desc&per_page=2"),
        answer(json!(listed).to_string(), None),
    );
    for name in ["pay-api", "pay-ui", "pay-web"] {
        for segment in ["code-scanning", "dependabot", "secret-scanning"] {
            let hidden = if segment == "secret-scanning" {
                "&hide_secret=true"
            } else {
                ""
            };
            let own = if (name, segment) == ("pay-api", "dependabot") {
                json!([alert(1, "o/pay-api")])
            } else {
                json!([])
            };
            held.insert(
                format!("{api}/repos/o/{name}/{segment}/alerts?state=open&per_page=100{hidden}"),
                answer(own.to_string(), None),
            );
        }
    }
    Script::keyed(held)
}

#[test]
fn a_view_that_holds_some_members_reads_the_first_by_name_whatever_those_it_hides_pushed() {
    let config = owned("advisories_from = \"members\"\nadvisory_repositories = 2");
    let store = listed_of(&["pay-web", "other", "pay-ui", "pay-api"]);
    let read = |pushed: &[&str]| {
        let forge = pushed_first(pushed);
        let board = read_with(&config, &forge, &store);
        (board, forge.seen())
    };
    let (hidden_first, seen) = read(&["other", "pay-web"]);
    let (held_first, _) = read(&["pay-web", "pay-ui"]);
    assert_eq!(format!("{hidden_first:?}"), format!("{held_first:?}"));
    assert!(
        seen.iter().all(|line| !line.contains("sort=pushed")),
        "{seen:?}"
    );
    let found = hidden_first.overviews.advisories.unwrap();
    let found = found.shown().unwrap();
    let read: Vec<&str> = found
        .open
        .iter()
        .map(|row| row.repository.as_str())
        .collect();
    assert_eq!(read, ["o/pay-api"]);
    assert!(found.more);
    let reasons: Vec<&str> = found
        .unread
        .iter()
        .map(|unread| unread.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        [
            "1 of the member repositories the view holds were not read: only the first 2 by name are, as advisory_repositories says"
        ]
    );
    assert!(
        seen.iter().any(|line| line.contains("/repos/o/pay-ui/")),
        "{seen:?}"
    );
    assert!(
        seen.iter().all(|line| !line.contains("/repos/o/pay-web/")),
        "{seen:?}"
    );
}

#[test]
fn a_member_the_view_hides_is_never_read_one_by_one() {
    let mut answers = searched();
    for segment in ["code-scanning", "dependabot", "secret-scanning"] {
        let hidden = if segment == "secret-scanning" {
            "&hide_secret=true"
        } else {
            ""
        };
        answers.push((
            format!("/repos/o/pay-api/{segment}/alerts?state=open&per_page=100{hidden}"),
            200,
            "[]".to_owned(),
        ));
    }
    let (board, script) = queued(&owned("advisories_from = \"shown\""), &listed(), &answers);
    assert!(
        script
            .seen()
            .iter()
            .all(|line| !line.contains("/repos/o/other/")),
        "{:?}",
        script.seen()
    );
    let found = board.overviews.advisories.unwrap();
    assert_eq!(found.shown().unwrap().unread, []);
}

#[test]
fn an_owner_s_member_the_view_hides_is_never_read() {
    let mut answers = searched();
    for segment in ["code-scanning", "dependabot", "secret-scanning"] {
        let hidden = if segment == "secret-scanning" {
            "&hide_secret=true"
        } else {
            ""
        };
        answers.push((
            format!("/repos/o/pay-api/{segment}/alerts?state=open&per_page=100{hidden}"),
            200,
            "[]".to_owned(),
        ));
    }
    let (board, script) = queued(&owned("advisories_from = \"members\""), &listed(), &answers);
    assert!(
        script
            .seen()
            .iter()
            .all(|line| !line.contains("/repos/o/other/")),
        "{:?}",
        script.seen()
    );
    let found = board.overviews.advisories.unwrap();
    assert_eq!(found.shown().unwrap().unread, []);
}

#[test]
fn a_one_repository_source_the_view_hides_shows_none_of_its_advisories_or_unread_listings() {
    let text = "version = 1\n[store]\nkind = \"sqlite\"\n[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\ntoken = { secret = \"HERALD_GITHUB_TOKEN\" }\n[views.security]\nsources = [\"open\"]\noverviews = [\"advisories\"]\nrepositories = [\"o/pay*\"]\n";
    let config = load(
        "herald.toml",
        text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let open = alerts(true);
    let answers = [
        (GRAPHQL.to_owned(), 200, page()),
        (CODE.to_owned(), 403, "{}".to_owned()),
        (DEPENDABOT.to_owned(), 200, open),
        (SECRETS.to_owned(), 200, "[]".to_owned()),
    ];
    let (board, script) = queued(&config, &healthy(), &answers);
    let found = board.overviews.advisories.unwrap();
    let found = found.shown().unwrap();
    assert_eq!(
        (found.open.len(), found.unread.len()),
        (0, 0),
        "{:?}",
        script.seen()
    );
    assert_eq!(script.seen().len(), 4);
}

/// The living board `live`, private, posted at noon into `store`.
pub(crate) fn boarded(store: &Failing) {
    let (page, open) = (page(), alerts(true));
    let first = Script::answering(&answers((&page, &open), &[POSTED]));
    round(
        &crate::advisories::config(&board(true)),
        (store, &first),
        (NOON, None),
    );
    assert!(posted(&first)[0].contains(ALERT_PAGE));
}

pub(crate) fn minute() -> Timestamp {
    NOON.checked_add(SignedDuration::from_mins(1)).unwrap()
}

pub(crate) fn recalled(swept: &[Swept]) -> Vec<&Outcome> {
    swept
        .iter()
        .filter_map(|swept| match &swept.outcome {
            Sweep::Reported {
                pulls: 0,
                announced,
                ..
            } => Some(announced),
            _ => None,
        })
        .flatten()
        .flat_map(|fresh| &fresh.announced)
        .map(|announced| &announced.outcome)
        .collect()
}

#[test]
fn a_living_post_whose_route_is_gone_or_no_longer_private_is_edited_to_the_fixed_line() {
    let gone = format!("{}{}", reported("live"), routed("live", "lobby", false));
    for later in [
        crate::advisories::config(&board(false)),
        crate::advisories::config(&gone),
    ] {
        let store = healthy();
        boarded(&store);
        let edited = Script::answering(&[EDITED]);
        let swept = round(&later, (&store, &edited), (minute(), None));
        assert_eq!(edited.seen().len(), 1, "{:?}", edited.seen());
        assert!(
            edited.seen()[0].starts_with("PATCH "),
            "{:?}",
            edited.seen()
        );
        let edit = posted(&edited).pop().unwrap();
        assert!(edit.contains(RECALLED), "{edit}");
        for held in [ALERT_PAGE, "Prototype pollution", "critical", "o/r", "live"] {
            assert!(!edit.contains(held), "{held}: {edit}");
        }
        assert_eq!(recalled(&swept).len(), 1);
        let quiet = Script::answering(&[]);
        let again = round(&later, (&store, &quiet), (minute(), None));
        assert_eq!(quiet.seen(), Vec::<String>::new());
        assert_eq!(recalled(&again), Vec::<&Outcome>::new());
    }
}

#[test]
fn a_living_post_still_private_is_left_alone_and_one_that_cannot_be_reached_kept_and_named() {
    let store = healthy();
    boarded(&store);
    let quiet = Script::answering(&[CHANNEL_READ, GUILD_READ, ONBOARDING_READ]);
    round(
        &crate::advisories::config(&board(true)),
        (&store, &quiet),
        (minute(), None),
    );
    assert_eq!(
        quiet.seen(),
        [CHANNEL_READ.0, GUILD_READ.0, ONBOARDING_READ.0]
            .map(|path| format!("GET https://discord.com/api/v10{path}"))
    );
    let unreached = crate::advisories::config(&reported("live"));
    for _ in 0..2 {
        let none = Script::answering(&[]);
        let swept = round(&unreached, (&store, &none), (minute(), None));
        assert_eq!(none.seen(), Vec::<String>::new());
        assert!(
            matches!(recalled(&swept)[..], [Outcome::Skipped(_)]),
            "{swept:?}"
        );
    }
    let found = pollster::block_on(recalls(
        &unreached,
        (&store.cursors(), &store.recalls(), &store.ledger()),
    ));
    assert_eq!(found.mistakes.len(), 1, "{found:?}");
    assert!(
        found.mistakes[0].contains("report live on the route live"),
        "{found:?}"
    );
    let named = pollster::block_on(recalls(
        &crate::advisories::config(&board(true)),
        (&store.cursors(), &store.recalls(), &store.ledger()),
    ));
    assert_eq!(named.mistakes, Vec::<String>::new());
}

#[test]
fn a_living_post_herald_cannot_record_first_is_never_sent_the_section() {
    let store = Failing {
        fault: Fault::WritesCursors,
        ..healthy()
    };
    let (page, open) = (page(), alerts(true));
    let script = Script::answering(&answers((&page, &open), &[POSTED]));
    round(
        &crate::advisories::config(&board(true)),
        (&store, &script),
        (NOON, None),
    );
    let body = posted(&script).pop().unwrap();
    assert!(body.contains(WITHHELD), "{body}");
    assert!(!body.contains(ALERT_PAGE), "{body}");
}

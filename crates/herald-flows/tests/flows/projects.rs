//! A route announcing each project as an actor of its own: each actor
//! served while its project is named, kept while followed after its
//! project leaves, gone after; follows, approvals, announcements and doctor
//! by project (ADR-0036).

use std::collections::BTreeMap;

use herald_config::{Config, Uses, load};
use herald_core::{DestinationName, Facts, Listed, Membership, Sent, SourceName, Step, Visibility};
use herald_flows::{
    Checking, Decision, FollowError, Followed, Inbound, Outcome, Ports, Verified, announce, decide,
    followed, followers, online, prepare,
};
use herald_ports::{Cursors, Notes, Subscriptions};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::{SignedDuration, Timestamp};

use crate::activitypub::follower;
use crate::harness::{Failing, Fault, Script, healthy, outcome};
use crate::inbound::at;
use crate::serving::{World, get, said};

const ANA: &str = "https://social.example/users/ana";
const ANA_INBOX: &str = "https://social.example/users/ana/inbox";
const SHARED: &str = "https://social.example/inbox";
const USERS: &str = "https://herald.example/users/";

fn config_text() -> String {
    r#"version = 1
[store]
kind = "sqlite"
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
block = ["blocked.example"]
[sources.org]
kind = "github"
owner = "nightworksio"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "projects"
sources = ["org", "gate", "ci"]
destination = "fedi"
template = "t"
actor = "{{ event.project | replace('.', '_') }}"
[[routes]]
id = "fixed"
sources = ["gate"]
destination = "fedi"
template = "t"
actor = "lemonfiber"
[apps.ci]
tokens = ["sha256:0000000000000000000000000000000000000000000000000000000000000000"]
projects = ["herald", "Night Works"]
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#
    .to_owned()
}

fn configured(text: &str) -> Config {
    load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn projects() -> Config {
    configured(&config_text())
}

/// The member `name` of `org`, of `visibility`, left at `left` where it
/// has.
fn member(
    store: &Failing,
    (id, name): (&str, &str),
    visibility: Visibility,
    left: Option<Timestamp>,
) {
    let membership = Membership {
        repository: Listed {
            id: id.to_owned(),
            name: name.to_owned(),
            path: format!("nightworksio/{name}"),
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        },
        discovered: at(),
        left,
        cursor: None,
        collides: None,
    };
    pollster::block_on(
        store
            .inner
            .put_member(&SourceName::new("org"), &membership, None),
    )
    .unwrap();
}

fn followed_by(store: &Failing, actor: &str, pending: Option<&str>) {
    let mut subscription = follower(actor, ANA, Some(SHARED));
    subscription.pending = pending.map(ToOwned::to_owned);
    pollster::block_on(store.inner.subscribe(&subscription)).unwrap();
}

/// The members every test keeps, as they stand at `now`.
fn members(store: &Failing, now: Timestamp) {
    let long_ago = now - SignedDuration::from_hours(24 * 40);
    member(store, ("1", "cli"), Visibility::Public, None);
    member(store, ("2", "gh.tools"), Visibility::Public, None);
    member(store, ("3", "old"), Visibility::Public, Some(now));
    member(store, ("4", "quiet"), Visibility::Public, Some(now));
    member(store, ("5", "ancient"), Visibility::Public, Some(long_ago));
    member(store, ("6", "secret"), Visibility::Private, None);
    member(store, ("7", "Upper"), Visibility::Public, None);
    let kept_for = SignedDuration::from_hours(24 * 30);
    member(
        store,
        ("8", "edge"),
        Visibility::Public,
        Some(now - kept_for),
    );
    member(store, ("9", "shunned"), Visibility::Public, Some(now));
    followed_by(store, "old", None);
    followed_by(store, "edge", None);
    let mut blocked = follower("shunned", "https://blocked.example/users/x", None);
    blocked.inbox = Some("https://blocked.example/inbox".to_owned());
    pollster::block_on(store.inner.subscribe(&blocked)).unwrap();
    followed_by(store, "quiet", Some("https://social.example/f/9"));
    followed_by(store, "ancient", None);
    followed_by(store, "renamed", None);
}

fn world_of(config: Config) -> World {
    let world = World::with(config, Script::default());
    members(&world.store, at());
    world
}

fn fingered(world: &World, resource: &str) -> u16 {
    let asked = Inbound {
        query: Some(resource),
        ..get("/.well-known/webfinger")
    };
    world.answer(&asked).reply.status
}

#[test]
fn each_project_s_actor_is_served_while_named_kept_while_followed_after_it_leaves_and_gone_after() {
    let world = world_of(projects());
    for (name, status) in [
        ("cli", 200),
        ("gh_tools", 200),
        ("php-mutation-gate", 200),
        ("herald", 200),
        ("lemonfiber", 200),
        ("old", 200),
        ("edge", 200),
        ("shunned", 410),
        ("quiet", 410),
        ("ancient", 410),
        ("renamed", 410),
        ("secret", 404),
        ("upper", 404),
        ("nobody", 404),
    ] {
        let path = format!("/users/{name}");
        assert_eq!(world.answer(&get(&path)).reply.status, status, "{name}");
        let account = format!("resource=acct:{name}@herald.example");
        assert_eq!(fingered(&world, &account), status, "{name}");
        let id = format!("resource={USERS}{name}");
        assert_eq!(fingered(&world, &id), status, "{name}");
    }
    assert_eq!(fingered(&world, "resource=acct:cli@elsewhere.example"), 404);
    let (status, _, body) = said(&world.answer(&get("/users/gh_tools")));
    assert_eq!(status, 200);
    let document: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(document["id"], format!("{USERS}gh_tools"));
    let (_, _, gone) = said(&world.answer(&get("/users/quiet/outbox")));
    assert_eq!(gone, "the actor is gone");
    let (status, _, counted) = said(&world.answer(&get("/users/old/followers")));
    assert_eq!(status, 200);
    let counted: serde_json::Value = serde_json::from_str(&counted).unwrap();
    assert_eq!(counted["totalItems"], 1);
}

#[test]
fn a_member_not_everyone_may_see_has_an_actor_only_on_a_private_route_or_where_doctor_allows_it() {
    for (changed, status) in [
        ("id = \"projects\"\nprivate = true", 200),
        ("id = \"projects\"", 404),
    ] {
        let text = config_text().replacen("id = \"projects\"", changed, 1);
        let world = world_of(configured(&text));
        assert_eq!(
            world.answer(&get("/users/secret")).reply.status,
            status,
            "{changed}"
        );
    }
    let allowed = format!("{}[doctor]\nprivate_to_public = \"allow\"\n", config_text());
    let world = world_of(configured(&allowed));
    assert_eq!(world.answer(&get("/users/secret")).reply.status, 200);
}

#[test]
fn an_actor_whose_project_s_members_cannot_be_read_is_unavailable() {
    let mut world = World::with(projects(), Script::default());
    world.store.fault = Fault::ReadsMembers;
    assert_eq!(world.answer(&get("/users/someone")).reply.status, 503);
    assert_eq!(
        fingered(&world, "resource=acct:someone@herald.example"),
        503
    );
    assert_eq!(world.answer(&get("/users/lemonfiber")).reply.status, 200);
}

fn served(answers: &[(&str, u16, &str)]) -> Script {
    Script::answering(answers).headed(&[("content-type", "application/activity+json")])
}

fn connected(config: &Config, script: &Script) -> herald_flows::Connected<Script> {
    connected_for(config, script, Uses::Announces)
}

fn connected_for(config: &Config, script: &Script, uses: Uses) -> herald_flows::Connected<Script> {
    let secrets = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, uses)).unwrap();
    prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap()
}

fn activity(kind: &str, actor: &str) -> Vec<u8> {
    let follow = serde_json::json!({
        "id": "https://social.example/f/1",
        "type": "Follow",
        "actor": ANA,
        "object": format!("{USERS}{actor}"),
    });
    match kind {
        "Undo" => serde_json::json!({ "id": "https://social.example/u/1", "type": "Undo", "actor": ANA, "object": follow }),
        _ => follow,
    }
    .to_string()
    .into_bytes()
}

fn taken(store: &Failing, script: &Script, body: &[u8]) -> Result<Followed, FollowError> {
    let now = at();
    pollster::block_on(followed(
        &connected(&projects(), script),
        store,
        &FakeSigner,
        Checking {
            now,
            bounds: &herald_core::Inbound::DEFAULT,
        },
        &Verified {
            delivery: None,
            signer: Some(ANA.to_owned()),
        },
        body,
    ))
}

fn ana() -> String {
    serde_json::json!({
        "id": ANA,
        "type": "Person",
        "inbox": ANA_INBOX,
        "endpoints": { "sharedInbox": SHARED },
    })
    .to_string()
}

#[test]
fn a_project_s_actor_takes_follows_while_named_rejects_them_once_kept_and_ignores_them_once_gone() {
    let store = healthy();
    members(&store, at());
    let document = ana();
    let script = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
    assert_eq!(
        taken(&store, &script, &activity("Follow", "cli")),
        Ok(Followed::Accepted {
            follower: ANA.to_owned(),
            actor: "cli".to_owned(),
            answered: Sent::Landed(Facts::default()),
        })
    );
    let accepted: serde_json::Value = serde_json::from_slice(&script.bodies()[1]).unwrap();
    assert_eq!(accepted["actor"], format!("{USERS}cli"));
    let script = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
    assert_eq!(
        taken(&store, &script, &activity("Follow", "old")),
        Ok(Followed::Rejected {
            follower: ANA.to_owned(),
            actor: "old".to_owned(),
            answered: Sent::Landed(Facts::default()),
        })
    );
    let rejected: serde_json::Value = serde_json::from_slice(&script.bodies()[1]).unwrap();
    assert_eq!(rejected["type"], "Reject");
    for gone in ["quiet", "nobody"] {
        assert_eq!(
            taken(&store, &served(&[]), &activity("Follow", gone)),
            Ok(Followed::Ignored(
                "it follows no actor herald serves".to_owned()
            ))
        );
    }
    assert_eq!(
        taken(&store, &served(&[]), &activity("Undo", "old")),
        Ok(Followed::Undone {
            follower: ANA.to_owned(),
            actor: "old".to_owned(),
        })
    );
    let left = pollster::block_on(
        store
            .inner
            .subscribers(&DestinationName::new("fedi"), "old"),
    )
    .unwrap();
    assert_eq!(left, []);
    assert_eq!(
        taken(&store, &served(&[]), &activity("Undo", "nobody")),
        Ok(Followed::Ignored(
            "it undoes a follow of no actor herald serves".to_owned()
        ))
    );
}

#[test]
fn a_waiting_follow_of_an_actor_that_takes_no_new_follows_can_only_be_rejected() {
    let store = healthy();
    members(&store, at());
    followed_by(&store, "old", Some("https://social.example/f/2"));
    let config = projects();
    let script = served(&[(SHARED, 202, "")]);
    let connected = connected(&config, &script);
    let fedi = DestinationName::new("fedi");
    let decided = |decision| {
        pollster::block_on(decide(
            &connected,
            &store,
            &FakeSigner,
            at(),
            &fedi,
            "old",
            ANA,
            decision,
        ))
    };
    assert_eq!(
        decided(Decision::Approve),
        Err(FollowError::Unknown(
            "\"old\" takes no new follows, as its project left its owner's selection; reject the follow".to_owned()
        ))
    );
    assert_eq!(
        pollster::block_on(followers(&connected, &store, &fedi, "old", at()))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        decided(Decision::Reject),
        Ok(Sent::Landed(Facts::default()))
    );
    assert_eq!(
        pollster::block_on(followers(&connected, &store, &fedi, "cli", at())).unwrap(),
        []
    );
}

fn announced(store: &Failing, script: &Script, project: &str) -> Vec<herald_flows::Announced> {
    announced_from(&projects(), store, script, ("ci", project))
}

fn announced_from(
    config: &Config,
    store: &Failing,
    script: &Script,
    (source, project): (&str, &str),
) -> Vec<herald_flows::Announced> {
    let connected = connected(config, script);
    let ports = Ports {
        store,
        clock: &FakeClock::at(at()),
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let event = herald_core::Event::normalise(herald_core::Draft {
        identity: "ci/1".to_owned(),
        kind: herald_core::Kind::RELEASE,
        project: project.to_owned(),
        attributes: herald_core::versioned("1.0.0".to_owned(), false),
        title: "1.0.0".to_owned(),
        body: "## Added".to_owned(),
        links: BTreeMap::new(),
        occurred: at(),
    })
    .unwrap();
    pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new(source),
        &event,
        "r1",
    ))
}

#[test]
fn an_event_is_announced_as_its_project_s_actor_to_that_actor_s_followers_alone() {
    let store = healthy();
    for actor in ["herald", "cli"] {
        pollster::block_on(store.inner.subscribe(&follower(
            actor,
            &format!("https://{actor}.example/users/ana"),
            Some(&format!("https://{actor}.example/inbox")),
        )))
        .unwrap();
    }
    let script = Script::answering(&[("https://herald.example/inbox", 202, "")]);
    let announced = announced(&store, &script, "herald");
    assert_eq!(
        outcome(&announced),
        &Outcome::Sent {
            steps: vec![
                Step::Post,
                Step::Deliver("https://herald.example/inbox".to_owned()),
            ]
        }
    );
    assert_eq!(script.seen(), ["POST https://herald.example/inbox"]);
    let delivered: serde_json::Value = serde_json::from_slice(&script.bodies()[0]).unwrap();
    assert_eq!(delivered["actor"], format!("{USERS}herald"));
    let listed = pollster::block_on(store.inner.outbox("herald", None, 1)).unwrap();
    assert_eq!(listed.total, 1);
}

#[test]
fn an_event_whose_project_names_no_actor_skips_the_route() {
    let store = healthy();
    let script = Script::default();
    let announced = announced(&store, &script, "Night Works");
    assert_eq!(announced.len(), 1);
    let Outcome::Skipped(reason) = outcome(&announced) else {
        panic!("the route is skipped");
    };
    assert_eq!(
        reason,
        "the project Night Works names no actor: \"Night Works\" is not an actor's name; give at most 30 lowercase letters, digits, `_` or a `-` within, such as \"php-mutation-gate\""
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn doctor_names_each_project_of_a_source_app_or_member_that_names_no_actor() {
    let text = format!(
        "{}[[routes]]\nid = \"apps\"\nsources = [\"ci\"]\ndestination = \"fedi\"\ntemplate = \"t\"\nactor = \"{{{{ event.project }}}}\"\n",
        config_text()
    );
    let config = configured(&text);
    let layer = herald_flows::Layer {
        name: "server",
        tier: herald_flows::Tier::Container,
        stores: vec!["sqlite"],
        endpoint: true,
        sessions: true,
        egress: herald_flows::Egress::Checked,
        rounds: herald_flows::Rounds::Workflow,
        round_secrets: &[],
        keyring: herald_flows::Memory::PerRequest,
    };
    let findings = herald_flows::doctor(
        &config,
        &layer,
        std::slice::from_ref(&layer),
        &herald_flows::Here::default(),
    );
    let unnamed: Vec<(&str, &str, &str)> = findings
        .gaps
        .iter()
        .filter(|gap| gap.key.ends_with("].actor"))
        .map(|gap| (gap.key.as_str(), gap.feature.as_str(), gap.why.as_str()))
        .collect();
    let rule = "\"Night Works\" is not an actor's name; give at most 30 lowercase letters, digits, `_` or a `-` within, such as \"php-mutation-gate\"; its events skip the route until the template names an actor for it";
    assert_eq!(
        unnamed,
        [
            (
                "routes[0].actor",
                "announcing Night Works on the route projects as an actor of its own",
                rule
            ),
            (
                "routes[2].actor",
                "announcing Night Works on the route apps as an actor of its own",
                rule
            )
        ]
    );
    let listing = crate::owner::repositories(&[(1, "cli"), (2, "Upper")]);
    let script = Script::answering(&[(crate::owner::LISTING, 200, &listing)]);
    let findings = pollster::block_on(online(&connected_for(&config, &script, Uses::Polls)));
    let unnamed: Vec<(&str, &str)> = findings
        .gaps
        .iter()
        .filter(|gap| gap.key.ends_with("].actor"))
        .map(|gap| (gap.key.as_str(), gap.why.as_str()))
        .collect();
    assert_eq!(
        unnamed,
        [(
            "routes[0].actor",
            "org's nightworksio/Upper: \"Upper\" is not an actor's name; give at most 30 lowercase letters, digits, `_` or a `-` within, such as \"php-mutation-gate\"; its events skip the route until the template names an actor for it"
        )]
    );
}

/// The config of [`projects`], with a second actor destination on the same
/// domain whose route's own actor is `lemonfiber` too where `twin` says so.
fn colliding(twin: bool) -> Config {
    let twin = if twin {
        "[destinations.twin]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = { secret = \"HERALD_ACTIVITYPUB_KEY\" }\n[[routes]]\nid = \"twin\"\nsources = [\"gate\"]\ndestination = \"twin\"\ntemplate = \"t\"\nactor = \"lemonfiber\"\n"
    } else {
        ""
    };
    configured(&format!("{}{twin}", config_text()))
}

/// The members whose projects name actors others name too.
fn twins(store: &Failing) {
    member(store, ("10", "web.site"), Visibility::Public, None);
    member(store, ("11", "web_site"), Visibility::Public, None);
    member(store, ("12", "lemonfiber"), Visibility::Public, None);
    member(store, ("13", "php-mutation-gate"), Visibility::Public, None);
}

const NONE_OF_THEM: &str = "so herald serves none of them";

#[test]
fn a_name_two_projects_name_is_served_as_neither_and_a_route_s_own_actor_keeps_its_name() {
    let world = World::with(colliding(false), Script::default());
    twins(&world.store);
    let (status, _, body) = said(&world.answer(&get("/users/web_site")));
    assert_eq!(
        (status, body.as_str()),
        (
            404,
            "the actor's name is held by org's project web.site, org's project web_site, so herald serves none of them"
        )
    );
    let (status, _, body) = said(&world.answer(&get("/users/php-mutation-gate")));
    assert_eq!(status, 404);
    assert!(body.contains("gate's project php-mutation-gate"), "{body}");
    assert!(body.contains("org's project php-mutation-gate"), "{body}");
    assert_eq!(
        fingered(&world, "resource=acct:web_site@herald.example"),
        404
    );
    let (status, _, body) = said(&world.answer(&get("/users/lemonfiber")));
    assert_eq!(status, 200);
    let document: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(document["id"], format!("{USERS}lemonfiber"));
    let world = World::with(colliding(true), Script::default());
    let (status, _, body) = said(&world.answer(&get("/users/lemonfiber")));
    assert_eq!(
        (status, body.as_str()),
        (
            404,
            "the actor's name is held by the actor of fedi, the actor of twin, so herald serves none of them"
        )
    );
}

#[test]
fn a_follow_or_an_approval_of_a_name_two_projects_name_is_refused() {
    let store = healthy();
    twins(&store);
    followed_by(&store, "web_site", Some("https://social.example/f/3"));
    assert_eq!(
        taken(&store, &served(&[]), &activity("Follow", "web_site")),
        Ok(Followed::Ignored(
            "the actor's name is held by org's project web.site, org's project web_site, so herald serves none of them".to_owned()
        ))
    );
    let config = projects();
    let connected = connected(&config, &served(&[]));
    let refused = pollster::block_on(decide(
        &connected,
        &store,
        &FakeSigner,
        at(),
        &DestinationName::new("fedi"),
        "web_site",
        ANA,
        Decision::Approve,
    ));
    assert!(
        matches!(&refused, Err(FollowError::Unknown(why)) if why.ends_with(NONE_OF_THEM)),
        "{refused:?}"
    );
}

#[test]
fn a_project_naming_an_actor_another_holds_skips_its_route() {
    let store = healthy();
    twins(&store);
    let config = colliding(false);
    let skipped = |source: &str, project: &str| {
        let announced = announced_from(&config, &store, &Script::default(), (source, project));
        announced
            .iter()
            .find(|announced| announced.route.as_str() == "projects")
            .map(|announced| announced.outcome.clone())
    };
    assert_eq!(
        skipped("org", "web.site"),
        Some(Outcome::Skipped(
            "the project web.site names the actor web_site, which org's project web_site holds too"
                .to_owned()
        ))
    );
    assert_eq!(
        skipped("org", "lemonfiber"),
        Some(Outcome::Skipped(
            "the project lemonfiber names the actor lemonfiber, which the actor of fedi holds too"
                .to_owned()
        ))
    );
    assert_eq!(
        skipped("gate", "php-mutation-gate"),
        Some(Outcome::Skipped(
            "the project php-mutation-gate names the actor php-mutation-gate, which org's project php-mutation-gate holds too".to_owned()
        ))
    );
    let fresh = healthy();
    twins(&fresh);
    let fixed = announced_from(
        &config,
        &fresh,
        &Script::default(),
        ("gate", "php-mutation-gate"),
    );
    assert!(
        fixed
            .iter()
            .any(|announced| announced.route.as_str() == "fixed"
                && matches!(announced.outcome, Outcome::Sent { .. })),
        "{fixed:?}"
    );
    let mut broken = healthy();
    broken.fault = Fault::ReadsMembers;
    let stopped = announced_from(&config, &broken, &Script::default(), ("ci", "herald"));
    assert!(
        matches!(
            outcome(&stopped),
            Outcome::Stopped {
                reason: herald_flows::Stop::Store(_),
                ..
            }
        ),
        "{stopped:?}"
    );
}

#[test]
fn doctor_names_each_actor_s_name_more_than_one_holds() {
    let text = config_text().replace(
        "projects = [\"herald\", \"Night Works\"]",
        "projects = [\"herald\", \"lemonfiber\", \"a.b\", \"a_b\"]",
    );
    let config = configured(&format!(
        "{text}[destinations.twin]\nkind = \"activitypub\"\ndomain = \"herald.example\"\nkey = {{ secret = \"HERALD_ACTIVITYPUB_KEY\" }}\n[[routes]]\nid = \"twin\"\nsources = [\"gate\"]\ndestination = \"twin\"\ntemplate = \"t\"\nactor = \"lemonfiber\"\n"
    ));
    let layer = herald_flows::Layer {
        name: "server",
        tier: herald_flows::Tier::Container,
        stores: vec!["sqlite"],
        endpoint: true,
        sessions: true,
        egress: herald_flows::Egress::Checked,
        rounds: herald_flows::Rounds::Workflow,
        round_secrets: &[],
        keyring: herald_flows::Memory::PerRequest,
    };
    let findings = herald_flows::doctor(
        &config,
        &layer,
        std::slice::from_ref(&layer),
        &herald_flows::Here::default(),
    );
    let actors: Vec<&str> = findings
        .mistakes
        .iter()
        .filter(|mistake| mistake.starts_with("The actor "))
        .map(String::as_str)
        .collect();
    assert_eq!(
        actors,
        [
            "The actor a_b@herald.example is named by ci's project a.b on routes[0], ci's project a_b on routes[0]; herald serves it as none of theirs, and announces no project naming it on its route; give each its own name.",
            "The actor lemonfiber@herald.example is named by routes[1].actor, routes[2].actor, ci's project lemonfiber on routes[0]; herald serves it as none of theirs, and announces no project naming it on its route; give each its own name.",
        ]
    );
    let single = configured(&config_text().replace(
        "projects = [\"herald\", \"Night Works\"]",
        "projects = [\"lemonfiber\"]",
    ));
    let findings = herald_flows::doctor(
        &single,
        &layer,
        std::slice::from_ref(&layer),
        &herald_flows::Here::default(),
    );
    assert!(
        findings.mistakes.contains(&"The actor lemonfiber@herald.example is named by routes[1].actor, ci's project lemonfiber on routes[0]; the route's own actor keeps it, and no project naming it is announced on its route; change the template or the route's actor.".to_owned()),
        "{:?}",
        findings.mistakes
    );
    let listing = crate::owner::repositories(&[(1, "php-mutation-gate"), (2, "cli")]);
    let script = Script::answering(&[(crate::owner::LISTING, 200, &listing)]);
    let findings = pollster::block_on(online(&connected_for(&projects(), &script, Uses::Polls)));
    let actors: Vec<&str> = findings
        .mistakes
        .iter()
        .filter(|mistake| mistake.starts_with("The actor "))
        .map(String::as_str)
        .collect();
    assert_eq!(
        actors,
        [
            "The actor php-mutation-gate@herald.example is named by gate's project php-mutation-gate on routes[0], org's nightworksio/php-mutation-gate on routes[0]; herald serves it as none of theirs, and announces no project naming it on its route; give each its own name."
        ]
    );
}

#[test]
fn a_name_one_route_holds_as_gone_is_served_by_the_route_that_holds_it_live() {
    let team = "[[routes]]\nid = \"team\"\nsources = [\"org\"]\ndestination = \"fedi\"\ntemplate = \"t\"\nactor = \"{{ event.project }}\"\nprivate = true\n";
    let world = World::with(
        configured(&format!("{}{team}", config_text())),
        Script::default(),
    );
    members(&world.store, at());
    followed_by(&world.store, "secret", None);
    assert_eq!(world.answer(&get("/users/secret")).reply.status, 200);
}

mod matching;

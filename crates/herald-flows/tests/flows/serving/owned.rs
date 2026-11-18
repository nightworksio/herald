//! An owner source's webhooks: each delivery of a repository the owner holds
//! announced only where the owner's last membership record holds it when
//! its work runs.

use std::collections::BTreeMap;

use herald_core::{Listed, Membership, SourceName, Visibility};
use herald_flows::Withheld;
use herald_ports::Cursors;

use super::*;

const ID: &str = "212613049";

fn owned_world() -> World {
    owned_world_with("")
}

fn owned_world_with(extra: &str) -> World {
    let text = config_text().replacen(
        "repository = \"cli/cli\"\nwebhook",
        &format!("owner = \"cli\"\n{extra}webhook"),
        1,
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    World::with(config, Script::default())
}

fn kept(world: &World, visibility: Visibility, left: bool) {
    kept_in(world, "gate", visibility, left);
}

fn kept_in(world: &World, source: &str, visibility: Visibility, left: bool) {
    let membership = Membership {
        repository: Listed {
            id: ID.to_owned(),
            name: "cli".to_owned(),
            path: "cli/cli".to_owned(),
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
        left: left.then(at),
        cursor: None,
        collides: None,
        hooked: false,
    };
    let gate = SourceName::new(source);
    let version = pollster::block_on(world.store.inner.member(&gate, ID))
        .unwrap()
        .map(|held| held.version);
    pollster::block_on(world.store.inner.put_member(&gate, &membership, version)).unwrap();
}

fn delivered(world: &World) -> Job {
    let signature = signed("hook-secret", EVENT.as_bytes());
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "release"),
    ];
    let answered = world.answer(&hooked("/hooks/gate", &headers, EVENT.as_bytes()));
    assert_eq!(
        said(&answered),
        (202, TEXT, "herald announces the release".to_owned())
    );
    let Some(job) = answered.job else {
        panic!("the release is left to announce");
    };
    let Job::Announce { member, .. } = &job else {
        panic!("a webhook's release is announced");
    };
    assert_eq!(
        member
            .as_ref()
            .map(|member| (member.id.as_str(), member.visibility)),
        Some((ID, Some(Visibility::Public)))
    );
    job
}

#[test]
fn a_private_delivery_of_a_public_member_stays_private_whether_or_not_its_source_expands() {
    let world = owned_world();
    let Job::Announce {
        source,
        event,
        member,
        scheme,
        verified,
    } = delivered(&world)
    else {
        panic!("a webhook's release is announced");
    };
    let private = Job::Announce {
        source,
        event: event.seen_as(Visibility::Private),
        member,
        scheme,
        verified,
    };
    let unexpanded = World::with(
        load(
            "herald.toml",
            &config_text(),
            &|_| Err(String::new()),
            &crate::harness::kinds(),
        )
        .unwrap(),
        Script::default(),
    );
    for world in [&world, &unexpanded] {
        kept(world, Visibility::Public, false);
        assert_eq!(world.work(private.clone()), Did::Announced(Vec::new()));
    }
}

#[test]
fn an_owner_s_webhook_is_announced_only_where_its_last_membership_record_holds_the_member() {
    let world = owned_world();
    let job = delivered(&world);
    assert_eq!(world.work(job.clone()), Did::Withheld(Withheld::Unlisted));
    kept(&world, Visibility::Public, true);
    assert_eq!(world.work(job.clone()), Did::Withheld(Withheld::Left));
    kept(&world, Visibility::Public, false);
    assert_eq!(
        world.work(job.clone()),
        Did::Announced(vec![herald_flows::Announced {
            route: RouteId::new("followed"),
            outcome: Outcome::Sent {
                steps: vec![Step::Post]
            },
        }])
    );
    kept(&world, Visibility::Private, false);
    assert_eq!(world.work(job.clone()), Did::Announced(Vec::new()));
    let gate = SourceName::new("gate");
    let held = pollster::block_on(world.store.inner.member(&gate, ID))
        .unwrap()
        .unwrap();
    let colliding = Membership {
        collides: Some("its project is the mutation source's".to_owned()),
        ..held.value
    };
    pollster::block_on(
        world
            .store
            .inner
            .put_member(&gate, &colliding, Some(held.version)),
    )
    .unwrap();
    assert_eq!(
        world.work(job),
        Did::Withheld(Withheld::Collides(
            "its project is the mutation source's".to_owned()
        ))
    );
}

#[test]
fn an_owner_s_webhook_whose_membership_cannot_be_read_releases_its_delivery() {
    let mut world = owned_world();
    let job = delivered(&world);
    world.store.fault = Fault::ReadsMembers;
    let Did::Withheld(Withheld::Unread(reason)) = world.work(job.clone()) else {
        panic!("a membership that cannot be read withholds the release");
    };
    assert_ne!(reason, String::new());
    assert!(Did::Withheld(Withheld::Unread(reason)).stopped());
    assert!(!Did::Withheld(Withheld::Left).stopped());
    let Job::Announce { verified, .. } = job else {
        panic!("a webhook's release is announced");
    };
    assert_eq!(
        world.admits(
            herald_sources_forge::Forge::GitHub.scheme().name,
            verified.delivery.as_deref().unwrap()
        ),
        Admitted::Fresh
    );
}

#[test]
fn an_owner_s_webhook_of_a_repository_it_does_not_take_announces_nothing() {
    let world = owned_world();
    let private = {
        let mut payload: serde_json::Value = serde_json::from_str(EVENT).unwrap();
        payload["repository"]["visibility"] = serde_json::json!("private");
        serde_json::to_vec(&payload).unwrap()
    };
    let signature = signed("hook-secret", &private);
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "release"),
    ];
    let answered = world.answer(&hooked("/hooks/gate", &headers, &private));
    assert_eq!(
        said(&answered),
        (
            200,
            TEXT,
            "the repository is not one the source takes".to_owned()
        )
    );
    assert_eq!(answered.job, None);
}

#[test]
fn an_owner_s_webhook_its_project_template_names_no_project_for_announces_nothing() {
    let world = owned_world_with("project = \"{{ repository.properties.team[0] }}\"\n");
    let job = delivered(&world);
    kept(&world, Visibility::Public, false);
    let Did::Withheld(Withheld::Unnamed(reason)) = world.work(job) else {
        panic!("a member without a project announces nothing");
    };
    assert!(reason.starts_with("template project: "), "{reason}");
}

#[test]
fn a_member_s_job_of_a_source_of_one_repository_keeps_its_project() {
    let world = World::new(Script::default());
    let event = herald_sources_forge::Forge::GitHub
        .event(EVENT.as_bytes(), herald_core::PrereleaseRule::default())
        .unwrap()
        .unwrap();
    kept_in(&world, "quiet", Visibility::Public, false);
    let did = world.work(Job::Announce {
        source: SourceName::new("quiet"),
        event,
        member: Some(herald_flows::Member {
            id: ID.to_owned(),
            visibility: Some(Visibility::Public),
        }),
        scheme: herald_sources_forge::Forge::GitHub.scheme(),
        verified: herald_flows::Verified {
            delivery: Some("d9".to_owned()),
            signer: None,
        },
    });
    assert_eq!(did, Did::Announced(Vec::new()));
}

#[test]
fn an_owner_s_webhook_of_a_member_its_record_names_no_project_for_announces_nothing() {
    let world = owned_world();
    let job = delivered(&world);
    kept(&world, Visibility::Public, false);
    let gate = SourceName::new("gate");
    let held = pollster::block_on(world.store.inner.member(&gate, ID))
        .unwrap()
        .unwrap();
    let nameless = Membership {
        repository: Listed {
            name: " ".to_owned(),
            ..held.value.repository.clone()
        },
        ..held.value
    };
    pollster::block_on(
        world
            .store
            .inner
            .put_member(&gate, &nameless, Some(held.version)),
    )
    .unwrap();
    assert_eq!(
        world.work(job),
        Did::Withheld(Withheld::Unnamed("the event names no project".to_owned()))
    );
}

#[test]
fn a_delivery_naming_its_repository_private_reaches_only_private_routes_though_its_record_says_public()
 {
    let world = owned_world();
    let Job::Announce {
        source,
        event,
        scheme,
        verified,
        ..
    } = delivered(&world)
    else {
        panic!("a webhook's release is announced");
    };
    kept(&world, Visibility::Public, false);
    let announced = |visibility: Option<Visibility>| {
        let Did::Announced(announced) = world.work(Job::Announce {
            source: source.clone(),
            event: event.clone(),
            member: Some(herald_flows::Member {
                id: ID.to_owned(),
                visibility,
            }),
            scheme,
            verified: verified.clone(),
        }) else {
            panic!("a member's release is announced where its routes allow");
        };
        announced
            .into_iter()
            .map(|announced| announced.route)
            .collect::<Vec<_>>()
    };
    assert_eq!(announced(Some(Visibility::Private)), Vec::<RouteId>::new());
    assert_eq!(announced(None), Vec::<RouteId>::new());
    assert_eq!(
        announced(Some(Visibility::Public)),
        [RouteId::new("followed")]
    );
}

/// The owner listing `gate` keeps, marked `stale`.
fn listed(world: &World, stale: bool) {
    let gate = SourceName::new("gate");
    let version = pollster::block_on(world.store.inner.listing(&gate))
        .unwrap()
        .map(|kept| kept.version);
    let listing = herald_core::Listing {
        owner: herald_core::Owner::Organization,
        pages: Vec::new(),
        first: at(),
        listed: at(),
        fingerprint: "f".to_owned(),
        file: None,
        events: None,
        stale,
    };
    pollster::block_on(world.store.inner.put_listing(&gate, &listing, version)).unwrap();
}

fn stale(world: &World) -> Option<bool> {
    pollster::block_on(world.store.inner.listing(&SourceName::new("gate")))
        .unwrap()
        .map(|kept| kept.value.stale)
}

#[test]
fn a_change_of_an_owner_s_repositories_marks_its_listing_stale_and_polls_it_at_once() {
    let mut world = owned_world();
    let payload = br#"{"action":"created","repository":{"id":7,"full_name":"cli/new"}}"#;
    let signature = signed("hook-secret", payload);
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "repository"),
    ];
    let answered = world.answer(&hooked("/hooks/gate", &headers, payload));
    assert_eq!(
        said(&answered),
        (
            202,
            TEXT,
            "herald lists the owner's repositories again".to_owned()
        )
    );
    let Some(job @ Job::Poll { relist: true, .. }) = answered.job else {
        panic!("a change of the owner's repositories is left to a poll");
    };
    let Did::Polled(swept) = world.work(job.clone()) else {
        panic!("the owner is polled");
    };
    assert!(swept.iter().all(|swept| swept.source.as_str() == "gate"));
    assert_eq!(stale(&world), None);
    listed(&world, false);
    world.work(job.clone());
    assert_eq!(stale(&world), Some(true));
    world.work(job.clone());
    assert_eq!(stale(&world), Some(true));
    listed(&world, false);
    let Job::Poll {
        source,
        scheme,
        verified,
        ..
    } = job.clone()
    else {
        unreachable!()
    };
    world.work(Job::Poll {
        source,
        scheme,
        verified,
        relist: false,
    });
    assert_eq!(stale(&world), Some(false));
    world.store.fault = Fault::ListingConflicts;
    assert_eq!(
        world.work(job),
        Did::Polled(vec![herald_flows::Swept {
            source: SourceName::new("gate"),
            member: None,
            outcome: herald_flows::Sweep::Stopped(herald_flows::Stop::Store(
                "another run moved the owner's listing first".to_owned()
            )),
        }])
    );
    assert_eq!(stale(&world), Some(false));
}

/// The owner source `gate`, a second owner source `mirror` of the same
/// owner, and the source `one` of the repository, each reading as the App
/// `herald`, whose webhooks the App takes.
fn installed_world() -> World {
    installed_world_beside(
        "[sources.mirror]\nkind = \"github\"\nowner = \"cli\"\napp = \"herald\"\n[sources.one]\nkind = \"github\"\nrepository = \"cli/cli\"\napp = \"herald\"\n",
    )
}

/// The owner source `gate` reading as the App `herald`, beside the sources
/// `beside`.
fn installed_world_beside(beside: &str) -> World {
    let text = config_text().replacen(
        "repository = \"cli/cli\"\nwebhook = { secret = \"HERALD_WEBHOOK\" }",
        &format!("owner = \"cli\"\napp = \"herald\"\n{beside}[github_apps.herald]\nclient_id = \"Iv23liABCDEF\"\nprivate_key = {{ secret = \"HERALD_APP_KEY\" }}\nwebhook = {{ secret = \"HERALD_WEBHOOK\" }}"),
        1,
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    World::with(config, Script::default())
}

#[test]
fn an_app_s_delivery_is_left_to_each_source_reading_as_the_app_that_takes_it() {
    let world = installed_world();
    let delivery = |path: &str, event: &str, body: &[u8]| {
        let signature = signed("hook-secret", body);
        let headers = [
            ("X-Hub-Signature-256", signature.as_str()),
            ("X-GitHub-Event", event),
            ("X-GitHub-Delivery", "d-app"),
        ];
        world.answer(&hooked(path, &headers, body))
    };
    let answered = delivery("/hooks/app/herald", "release", EVENT.as_bytes());
    assert_eq!(
        said(&answered),
        (
            202,
            TEXT,
            "herald takes the delivery for each source the App reaches".to_owned()
        )
    );
    let Some(Job::Fanned { jobs, .. }) = &answered.job else {
        panic!("the delivery is left to each source");
    };
    let sources: Vec<&str> = jobs
        .iter()
        .map(|job| match job {
            Job::Announce { source, .. } | Job::Poll { source, .. } => source.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(sources, ["gate", "mirror", "one"]);
    let Did::Fanned(each) = world.work(answered.job.unwrap()) else {
        panic!("each source's job is done");
    };
    assert_eq!(each.len(), 3);
    let moved = br#"{"action":"created","repository":{"id":7,"full_name":"cli/new"}}"#;
    let answered = delivery("/hooks/app/herald", "repository", moved);
    assert!(matches!(&answered.job, Some(Job::Fanned { jobs, .. }) if jobs.len() == 2));
    let elsewhere = br#"{"action":"created","repository":{"id":7,"full_name":"neovim/new"}}"#;
    assert_eq!(
        said(&delivery("/hooks/app/herald", "repository", elsewhere)),
        (
            200,
            TEXT,
            "no source the App reaches takes the delivery".to_owned()
        )
    );
    assert_eq!(
        said(&delivery("/hooks/app/other", "release", EVENT.as_bytes())).0,
        404
    );
    let answer = world.answer(&get("/hooks/app/herald"));
    assert_eq!(said(&answer).0, 405);
    let unsigned = world.answer(&hooked(
        "/hooks/app/herald",
        &[("X-GitHub-Event", "release")],
        EVENT.as_bytes(),
    ));
    assert_eq!(said(&unsigned).0, 401);
}

#[test]
fn an_app_s_delivery_one_source_takes_is_that_source_s_job() {
    let world = installed_world_beside("");
    let signature = signed("hook-secret", EVENT.as_bytes());
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "release"),
        ("X-GitHub-Delivery", "d-one"),
    ];
    let answered = world.answer(&hooked("/hooks/app/herald", &headers, EVENT.as_bytes()));
    assert_eq!(said(&answered).0, 202);
    assert!(
        matches!(&answered.job, Some(Job::Announce { source, .. } | Job::Poll { source, .. }) if source.as_str() == "gate"),
        "{:?}",
        answered.job
    );
}

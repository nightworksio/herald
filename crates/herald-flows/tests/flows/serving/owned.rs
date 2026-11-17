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

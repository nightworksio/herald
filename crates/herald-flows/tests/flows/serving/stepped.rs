//! A webhook of a source whose merge queues `herald server` steps wakes
//! them, and the work it leaves steps them (ADR-0087).

use herald_config::{Config, load};
use herald_flows::{Did, Job};

use super::{EVENT, TEXT, World, config_text, hooked, said, signed};
use crate::harness::Script;

/// The config, with a merge queue on `gate` whose steps the server takes.
fn served_config() -> Config {
    let text = format!(
        "{}[merge_queues.gate]\nsource = \"gate\"\nbranch = \"main\"\nmethod = \"squash\"\nwrite = {{ token = {{ secret = \"HERALD_MERGE\" }} }}\napplied_by = \"server\"\n",
        config_text()
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

#[test]
fn a_webhook_of_a_source_whose_queues_the_server_steps_wakes_them() {
    let signature = signed("hook-secret", EVENT.as_bytes());
    let ping = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "ping"),
    ];
    let release = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "release"),
    ];
    let unstepped = World::with(served_config(), Script::default());
    let quiet = unstepped.answer(&hooked("/hooks/gate", &ping, EVENT.as_bytes()));
    assert_eq!((quiet.reply.status, quiet.job), (200, None));
    let world = World::with(served_config(), Script::default()).stepping();
    let woken = world.answer(&hooked("/hooks/gate", &ping, EVENT.as_bytes()));
    assert_eq!(
        said(&woken),
        (
            202,
            TEXT,
            "herald reads the source's merge queues and takes the steps due".to_owned()
        )
    );
    let Some(step @ Job::Step { .. }) = woken.job else {
        panic!("the delivery wakes the queues");
    };
    assert!(step.delivery().is_some());
    let Did::Stepped(read) = world.work(step.clone()) else {
        panic!("the queues are stepped");
    };
    assert_eq!(
        read.iter()
            .map(|read| read.name.as_str())
            .collect::<Vec<_>>(),
        ["gate"]
    );
    assert!(!Did::Stepped(read).stopped());
    assert_eq!(unstepped.work(step), Did::Stepped(Vec::new()));
    let fresh = World::with(served_config(), Script::default()).stepping();
    let announced = fresh.answer(&hooked("/hooks/gate", &release, EVENT.as_bytes()));
    assert_eq!(
        said(&announced).2,
        "herald announces the release".to_owned()
    );
    let Some(Job::Fanned { jobs, .. }) = announced.job else {
        panic!("the release is announced and the queues woken");
    };
    assert!(
        matches!(&jobs[..], [Job::Announce { .. }, Job::Step { .. }]),
        "{jobs:?}"
    );
}

//! A delivery's change of a pull request a view reads kept, folded into
//! its record (ADR-0061).

use herald_core::{Checks, PullKey, RepositoryKey, SourceName};
use herald_ports::Pulls;

use super::*;

const OPENED: &str = r#"{"action":"opened","pull_request":{"number":7,"title":"Add a queue","html_url":"https://github.com/cli/cli/pull/7","state":"open","user":{"login":"octo","type":"User"},"draft":false,"created_at":"2026-11-18T10:00:00Z","updated_at":"2026-11-18T11:00:00Z","head":{"sha":"abc","ref":"feature"},"base":{"ref":"main"},"labels":[],"requested_reviewers":[]},"repository":{"id":1,"name":"cli","full_name":"cli/cli","html_url":"https://github.com/cli/cli","private":false}}"#;

const FAILED: &str = r#"{"action":"completed","check_run":{"name":"test","head_sha":"abc","status":"completed","conclusion":"failure","started_at":"2026-11-18T11:00:00Z","completed_at":"2026-11-18T11:05:00Z"},"repository":{"id":1,"name":"cli","full_name":"cli/cli","html_url":"https://github.com/cli/cli","private":false}}"#;

fn repository() -> RepositoryKey {
    RepositoryKey {
        host: "github.com".to_owned(),
        id: "1".to_owned(),
    }
}

fn delivered(world: &World, event: &str, body: &str) -> Answered {
    let signature = signed("hook-secret", body.as_bytes());
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", event),
    ];
    world.answer(&hooked("/hooks/gate", &headers, body.as_bytes()))
}

#[test]
fn a_delivery_of_a_pull_request_a_view_reads_kept_is_folded_into_its_record() {
    let world = World::new(Script::default());
    let answered = delivered(&world, "pull_request", OPENED);
    assert_eq!(
        said(&answered),
        (
            202,
            TEXT,
            "herald keeps the pull request's change".to_owned()
        )
    );
    let Some(job @ Job::Fold { .. }) = answered.job else {
        panic!("the change is left to fold");
    };
    assert_eq!(world.work(job), Did::Folded(Ok(1)));
    let key = PullKey {
        repository: repository(),
        number: 7,
    };
    let kept = pollster::block_on(world.store.pull(&key)).unwrap().unwrap();
    assert_eq!(kept.value.pull.author.as_deref(), Some("octo"));
    let mark = pollster::block_on(world.store.reconciled(&SourceName::new("gate")))
        .unwrap()
        .unwrap();
    assert_eq!(mark.value.repository, Some(repository()));
    let Some(job) = delivered(&world, "check_run", FAILED).job else {
        panic!("the check is left to fold");
    };
    assert_eq!(world.work(job), Did::Folded(Ok(1)));
    let kept = pollster::block_on(world.store.pull(&key)).unwrap().unwrap();
    assert_eq!(
        kept.value.pull.checks,
        Checks::Failed(vec!["test".to_owned()])
    );
    let Some(again) = delivered(&world, "check_run", &FAILED.replace("action", "act")).job else {
        panic!("the same check is left to fold");
    };
    assert_eq!(world.work(again), Did::Folded(Ok(0)));
    let moved = FAILED.replace("\"head_sha\":\"abc\"", "\"head_sha\":\"def\"");
    let Some(earlier) = delivered(&world, "check_run", &moved).job else {
        panic!("a check of another head is left to fold");
    };
    assert_eq!(world.work(earlier), Did::Folded(Ok(0)));
}

#[test]
fn a_delivery_names_the_repository_a_listing_did_not_and_keeps_its_validators() {
    let world = World::new(Script::default());
    let validators = vec![herald_core::Validators {
        etag: Some("\"e\"".to_owned()),
        last_modified: None,
    }];
    pollster::block_on(world.store.put_reconciled(
        &SourceName::new("gate"),
        &herald_core::Reconciled {
            repository: None,
            at: None,
            validators: validators.clone(),
            ..herald_core::Reconciled::default()
        },
        None,
    ))
    .unwrap();
    let Some(job) = delivered(&world, "pull_request", OPENED).job else {
        panic!("the change is left to fold");
    };
    assert_eq!(world.work(job), Did::Folded(Ok(1)));
    let mark = pollster::block_on(world.store.reconciled(&SourceName::new("gate")))
        .unwrap()
        .unwrap();
    assert_eq!(
        (mark.value.repository, mark.value.validators),
        (Some(repository()), validators)
    );
}

#[test]
fn a_delivery_announced_and_folded_is_left_to_both() {
    let text = config_text().replace(
        "webhook = { secret = \"HERALD_WEBHOOK\" }\n",
        "webhook = { secret = \"HERALD_WEBHOOK\" }\nsignals = [\"pull-request\"]\n",
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let world = World::with(config, Script::default());
    let answered = delivered(&world, "pull_request", OPENED);
    let Some(Job::Fanned { jobs, .. }) = answered.job else {
        panic!("the delivery is left to two jobs: {answered:?}");
    };
    assert!(matches!(
        jobs.as_slice(),
        [Job::Announce { .. }, Job::Fold { .. }]
    ));
}

#[test]
fn a_change_that_cannot_be_kept_releases_its_delivery_and_says_why() {
    for fault in [
        Fault::RacesPulls,
        Fault::ReadsReconciles,
        Fault::WritesReconciles,
    ] {
        let mut world = World::new(Script::default());
        let answered = delivered(&world, "pull_request", OPENED);
        world.store.fault = fault;
        let Some(job) = answered.job else {
            panic!("the change is left to fold");
        };
        assert!(matches!(world.work(job), Did::Folded(Err(_))), "{fault:?}");
    }
    let world = World::new(Script::default());
    pollster::block_on(world.store.put_reconciled(
        &SourceName::new("gate"),
        &herald_core::Reconciled {
            repositories: std::collections::BTreeSet::new(),
            repository: Some(repository()),
            at: None,
            validators: Vec::new(),
        },
        None,
    ))
    .unwrap();
    let Some(job) = delivered(&world, "check_run", FAILED).job else {
        panic!("the check is left to fold");
    };
    assert_eq!(world.work(job), Did::Folded(Ok(0)));
    let Some(job) = delivered(&world, "pull_request", OPENED).job else {
        panic!("the change is left to fold");
    };
    let mut unkept = World::new(Script::default());
    unkept.store.fault = Fault::KeepsNoPulls;
    assert_eq!(unkept.work(job), Did::Folded(Ok(0)));
}

#[test]
fn a_delivery_of_another_repository_is_refused_unread_and_a_live_view_folds_nothing() {
    let other = OPENED.replace("\"full_name\":\"cli/cli\"", "\"full_name\":\"cli/other\"");
    let answered = delivered(&World::new(Script::default()), "pull_request", &other);
    assert_eq!(answered.job, None);
    assert_ne!(answered.reply.status, 202);
    let live = config_text().replace(
        "[sources.quiet]",
        "[views.queue]\nmode = \"live\"\n[sources.quiet]",
    );
    let config = load(
        "herald.toml",
        &live,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let world = World::with(config, Script::default());
    let answered = delivered(&world, "pull_request", OPENED);
    assert_eq!(answered.job, None);
    assert_eq!(answered.reply.status, 200);
}

#[test]
fn a_repository_made_private_hides_every_pull_request_of_it_and_one_made_public_waits_for_a_read() {
    let world = World::new(Script::default());
    let Some(job) = delivered(&world, "pull_request", OPENED).job else {
        panic!("the change is left to fold");
    };
    assert_eq!(world.work(job), Did::Folded(Ok(1)));
    let key = PullKey {
        repository: repository(),
        number: 7,
    };
    let seen = || {
        pollster::block_on(world.store.pull(&key))
            .unwrap()
            .unwrap()
            .value
            .pull
            .visibility
    };
    assert_eq!(seen(), herald_core::Visibility::Private);
    let mut record = pollster::block_on(world.store.pull(&key)).unwrap().unwrap();
    record.value.pull.visibility = herald_core::Visibility::Public;
    pollster::block_on(world.store.put_pull(&record.value, Some(record.version))).unwrap();
    let made = |private: bool| {
        format!(
            r#"{{"action":"{}","repository":{{"id":1,"name":"cli","full_name":"cli/cli","html_url":"https://github.com/cli/cli","private":{private}}}}}"#,
            if private { "privatized" } else { "publicized" }
        )
    };
    let Some(job) = delivered(&world, "repository", &made(true)).job else {
        panic!("the repository's change is left to fold");
    };
    assert_eq!(world.work(job), Did::Folded(Ok(1)));
    assert_eq!(seen(), herald_core::Visibility::Private);
    let Some(job) = delivered(&world, "repository", &made(false)).job else {
        panic!("the repository's change is left to fold");
    };
    assert_eq!(world.work(job), Did::Folded(Ok(0)));
    assert_eq!(seen(), herald_core::Visibility::Private);
}

/// A world whose source `gate` names the owner `cli`, holding the member
/// `cli/cli` where `held` says, and since then left where `left` says.
fn owned(held: bool, left: bool) -> World {
    let text = config_text().replace(
        "[sources.gate]\nkind = \"github\"\nrepository = \"cli/cli\"\n",
        "[sources.gate]\nkind = \"github\"\nowner = \"cli\"\n",
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let world = World::with(config, Script::default());
    if held {
        let membership = herald_core::Membership {
            repository: herald_core::Listed {
                id: "1".to_owned(),
                name: "cli".to_owned(),
                path: "cli/cli".to_owned(),
                topics: Vec::new(),
                properties: std::collections::BTreeMap::new(),
                visibility: herald_core::Visibility::Public,
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
        pollster::block_on(herald_ports::Cursors::put_member(
            &world.store,
            &SourceName::new("gate"),
            &membership,
            None,
        ))
        .unwrap();
    }
    world
}

/// What working the fold the delivery `body` leaves did in `world`.
fn folded_in(world: &World, body: &str) -> Did {
    let answered = delivered(world, "pull_request", body);
    let said = said(&answered);
    let job = match answered.job {
        Some(job @ Job::Fold { .. }) => job,
        Some(Job::Fanned { mut jobs, .. }) => jobs.pop().unwrap(),
        other => panic!("the change is left to fold: {other:?}, {said:?}"),
    };
    world.work(job)
}

#[test]
fn an_owner_folds_a_delivery_only_of_a_member_its_membership_record_holds() {
    assert_eq!(folded_in(&owned(false, false), OPENED), Did::Folded(Ok(0)));
    assert_eq!(folded_in(&owned(true, true), OPENED), Did::Folded(Ok(0)));
    let world = owned(true, false);
    assert_eq!(folded_in(&world, OPENED), Did::Folded(Ok(1)));
    let key = PullKey {
        repository: repository(),
        number: 7,
    };
    let kept = pollster::block_on(world.store.pull(&key)).unwrap().unwrap();
    assert_eq!(kept.value.pull.project, "cli");
    assert_eq!(kept.value.pull.visibility, herald_core::Visibility::Private);
    let mark = pollster::block_on(world.store.reconciled(&SourceName::new("gate")))
        .unwrap()
        .unwrap();
    assert_eq!(mark.value.repository, None);
    assert!(mark.value.repositories.contains(&repository()));
    let again = OPENED.replace("\"action\":\"opened\"", "\"action\":\"edited\"");
    assert_eq!(folded_in(&world, &again), Did::Folded(Ok(0)));
}

use herald_core::SourceName;
use herald_flows::{Did, Job, Sweep};

use super::*;

const CREATED: &str = r#"{"ref":"v9","ref_type":"tag","repository":{"id":1,"name":"cli","full_name":"cli/cli","html_url":"https://github.com/cli/cli"}}"#;

#[test]
fn a_signed_webhook_of_a_tag_polls_its_source_alone() {
    let text = config_text().replace(
        "webhook = { secret = \"HERALD_WEBHOOK\" }\n",
        "webhook = { secret = \"HERALD_WEBHOOK\" }\nsignals = [\"tag\"]\n",
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let script = Script::answering(&[
        ("/repos/cli/cli/tags?per_page=30", 200, "[]"),
        (
            "/repos/cli/cli",
            200,
            r#"{"id":1,"name":"cli","html_url":"https://github.com/cli/cli"}"#,
        ),
    ]);
    let world = World::with(config, script);
    let signature = signed("hook-secret", CREATED.as_bytes());
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "create"),
    ];
    let answered = world.answer(&hooked("/hooks/gate", &headers, CREATED.as_bytes()));
    assert_eq!(
        said(&answered),
        (202, TEXT, "herald polls the source for the tag".to_owned())
    );
    let Some(job @ Job::Poll { .. }) = answered.job else {
        panic!("the tag is left to a poll");
    };
    let Did::Polled(swept) = world.work(job) else {
        panic!("the source is polled");
    };
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].source, SourceName::new("gate"));
    assert!(matches!(
        swept[0].outcome,
        Sweep::Polled { first: true, .. }
    ));
    let branch = CREATED.replace("\"tag\"", "\"branch\"");
    let signature = signed("hook-secret", branch.as_bytes());
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "create"),
    ];
    let answered = world.answer(&hooked("/hooks/gate", &headers, branch.as_bytes()));
    assert_eq!(
        said(&answered),
        (
            200,
            TEXT,
            "the delivery is not of a release or a tag".to_owned()
        )
    );
}

#[test]
fn a_poll_a_webhook_asked_for_that_stops_releases_its_delivery() {
    let did = Did::Polled(vec![herald_flows::Swept {
        source: SourceName::new("gate"),
        member: None,
        outcome: Sweep::Stopped(Stop::Source("down".to_owned())),
    }]);
    assert!(did.stopped());
    assert!(!Did::Polled(Vec::new()).stopped());
}

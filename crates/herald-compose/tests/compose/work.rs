//! Work kept within its host's limits.

use super::*;

fn announced_within(hosting: herald_compose::Hosting, store: &str, work: &str) -> String {
    let text = format!(
        r#"version = 1
[store]
{store}
[server.work]
{work}
[destinations.chat]
kind = "slack"
token = {{ secret = "TOKEN" }}
[[routes]]
id = "deployed"
sources = ["ci"]
destination = "chat"
channel = "C0123456789"
template = "t"
[apps.ci]
tokens = ["sha256:{}"]
projects = ["herald"]
routes = ["deployed"]
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#,
        herald_core::digest("app-token")
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &herald_compose::kinds(),
    )
    .unwrap();
    let host = Hosting {
        secrets: github().secrets.with("TOKEN", "t"),
        ..Hosting::default()
    };
    let composed =
        pollster::block_on(Composed::new(host, config, hosting, Uses::Serves, "test")).unwrap();
    let headers = [("Authorization", "Bearer app-token")];
    let inbound = Inbound {
        method: "POST",
        path: "/api/projects/herald/events",
        query: None,
        headers: &headers,
        body: br#"{"id": "1", "project": "herald", "title": "Deployed"}"#,
    };
    let answered = pollster::block_on(composed.answer(&inbound, Admission::Once));
    let left = answered.left.expect("the event's work");
    let Did::Announced(announced) = pollster::block_on(composed.work(left)).did else {
        panic!("an announcement");
    };
    format!("{:?}", announced[0].outcome)
}

#[test]
fn work_starts_no_send_its_host_s_limits_leave_no_room_for() {
    const DYNAMODB: &str = "kind = \"dynamodb\"";
    let worker = herald_compose::Hosting::Worker;
    let fastly = herald_compose::Hosting::Fastly;
    let room = announced_within(worker, DYNAMODB, "spare = \"29s\"");
    assert!(!room.contains("Limit("), "{room}");
    let counted = announced_within(fastly, DYNAMODB, "spare_requests = 31");
    assert!(
        counted.contains("of the 32 outbound requests the host allows were made")
            && !counted.contains("\"0 of"),
        "{counted}"
    );
    let room = announced_within(fastly, DYNAMODB, "spare_requests = 2");
    assert!(!room.contains("Limit("), "{room}");
    for (hosting, store, work) in [
        (worker, DYNAMODB, "after = \"40s\"\nspare = \"35s\""),
        (fastly, DYNAMODB, "requests = 100\nspare_requests = 40"),
        (
            NODE,
            "kind = \"deno-kv\"",
            "requests = 1\nspare_requests = 0",
        ),
    ] {
        let room = announced_within(hosting, store, work);
        assert!(!room.contains("Limit("), "{work}: {room}");
    }
}

#[test]
fn a_herald_whose_spare_leaves_its_host_s_limits_no_room_is_not_composed() {
    for (hosting, work, said) in [
        (
            herald_compose::Hosting::Worker,
            "spare = \"30s\"\n",
            "[server.work] spare, 30s, leaves no time for a send in the 30s the work may run after its answer; make it shorter",
        ),
        (
            herald_compose::Hosting::Fastly,
            "spare_requests = 32\n",
            "[server.work] spare_requests, 32, leaves no room for a send in the 32 outbound requests a request may make; make it fewer",
        ),
    ] {
        let text = format!("version = 1\n[store]\nkind = \"dynamodb\"\n[server.work]\n{work}");
        let config = load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &herald_compose::kinds(),
        )
        .unwrap();
        let refused = pollster::block_on(Composed::new(
            github(),
            config,
            hosting,
            Uses::Serves,
            "test",
        ))
        .err();
        assert_eq!(refused.as_deref(), Some(said));
    }
}

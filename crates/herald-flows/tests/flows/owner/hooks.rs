use herald_core::SecretRef;
use herald_sources_forge::MemberHooks;

use super::*;

const HOOKED: &str = "https://herald.example/hooks/gate";

/// The owner source registering a webhook on each member each round.
fn hooking() -> Config {
    let mut config = owned(|options| {
        options.member_hooks = Some(MemberHooks::Round);
        options.webhook = Some(SecretRef {
            secret: "HERALD_APP_KEY".to_owned(),
        });
    });
    config.server.url = Some("https://herald.example/".to_owned());
    config
}

fn hooks(id: u64) -> String {
    format!("/projects/{id}/hooks?per_page=100")
}

#[test]
fn each_member_without_a_webhook_to_herald_is_given_one_once() {
    let store = healthy();
    let config = hooking();
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let (herald, gate) = (
        releases("herald", &[("v1", NOON)]),
        releases("gate", &[("v1", NOON)]),
    );
    let found = json!([{ "url": HOOKED }]).to_string();
    let script = Script::answering(&[
        (LISTING, 200, &two),
        (&hooks(1), 200, "[]"),
        ("/projects/1/hooks", 201, "{}"),
        (&hooks(2), 200, &found),
        (&released("herald"), 200, &herald),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
        (&released("gate"), 200, &gate),
        ("/repos/nightworksio/gate", 200, &described(2, "gate")),
    ]);
    assert_eq!(
        polled(&config, &store, &script, NOON),
        [
            listed(2, &["herald", "gate"], &[]),
            member("herald", first(1)),
            member("gate", first(1)),
        ]
    );
    assert!(members(&store).iter().all(|member| member.hooked));
    let later = NOON + SignedDuration::from_hours(1);
    let quiet = Script::answering(&[
        (LISTING, 200, &two),
        (&released("herald"), 304, ""),
        (&released("gate"), 304, ""),
    ]);
    assert_eq!(
        polled(&config, &store, &quiet, later),
        [
            listed(2, &[], &[]),
            member("herald", Sweep::Unchanged),
            member("gate", Sweep::Unchanged),
        ]
    );
}

#[test]
fn a_member_that_cannot_be_given_a_webhook_says_why_and_is_still_polled() {
    let store = healthy();
    let config = hooking();
    let one = repositories(&[(1, "herald")]);
    let herald = releases("herald", &[("v1", NOON)]);
    let script = Script::answering(&[
        (LISTING, 200, &one),
        (&hooks(1), 403, "{}"),
        (&released("herald"), 200, &herald),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
    ]);
    let swept = polled(&config, &store, &script, NOON);
    assert_eq!(swept[1], listed(1, &["herald"], &[]));
    assert!(matches!(
        &swept[0],
        (Some(path), Sweep::Stopped(Stop::Source(reason)))
            if path == "nightworksio/herald" && reason.contains("403")
    ));
    assert_eq!(swept[2], member("herald", first(1)));
    assert!(!members(&store)[0].hooked);
}

#[test]
fn a_round_near_its_host_s_request_limit_registers_no_webhook() {
    let store = healthy();
    let config = hooking();
    let one = repositories(&[(1, "herald")]);
    let script = Script::answering(&[(LISTING, 200, &one)]);
    polled_within(&config, &store, &script, NOON, Some(2));
    assert!(!members(&store)[0].hooked);
}

#[test]
fn a_round_without_herald_s_address_registers_no_webhook() {
    let store = healthy();
    let mut config = hooking();
    config.server.url = None;
    let one = repositories(&[(1, "herald")]);
    let herald = releases("herald", &[("v1", NOON)]);
    let script = Script::answering(&[
        (LISTING, 200, &one),
        (&released("herald"), 200, &herald),
        ("/repos/nightworksio/herald", 200, &described(1, "herald")),
    ]);
    polled(&config, &store, &script, NOON);
    assert!(!members(&store)[0].hooked);
}

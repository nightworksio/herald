use super::world::*;
use crate::*;

fn config() -> String {
    format!(
        r#"{CONFIG}[[routes]]
id = "deploys"
sources = ["ci"]
destination = "discord"
channel = "200"
template = "release"
[[routes]]
id = "elsewhere"
sources = ["ci"]
destination = "discord"
channel = "300"
template = "release"
[apps.ci]
projects = ["herald"]
routes = ["deploys"]
"#
    )
}

fn written(setup: &Setup, name: &str, event: &str) -> String {
    let path = setup.directory.path().join(name);
    std::fs::write(&path, event).unwrap();
    path.display().to_string()
}

const DEPLOYED: &str =
    r#"{"id": "7", "project": "herald", "title": "Deployed", "body": "To production"}"#;

#[test]
fn a_new_token_is_printed_once_with_the_hash_herald_toml_keeps_and_needs_no_config() {
    let setup = setup("not toml");
    let made = herald(&setup, &World::default(), &["token", "new", "--app", "ci"]);
    assert_eq!(made.code, DONE, "{}", made.err);
    let mut lines = made.out.lines();
    let token = lines
        .next()
        .and_then(|line| line.strip_prefix("The app ci's token: "))
        .unwrap();
    assert_eq!(token.len(), 64);
    assert_eq!(
        lines.next(),
        Some(
            "Give it to the app as its bearer token, and keep its hash in herald.toml under [apps.ci]:"
        )
    );
    assert_eq!(
        lines.next().unwrap(),
        format!("tokens = [\"sha256:{}\"]", herald_core::digest(token))
    );
    assert!(
        made.out.ends_with("keeps nothing but its hash.\n"),
        "{}",
        made.out
    );
    let mut err = Vec::new();
    assert_eq!(apps::token("ci", None, &mut Vec::new(), &mut err), STOPPED);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "the system's random generator failed, so no token was made\n"
    );
}

#[test]
fn an_app_s_event_is_announced_to_the_routes_the_app_may_announce_to_only() {
    let setup = setup(&config());
    let world = World::default();
    let event = written(&setup, "deployed.json", DEPLOYED);
    let announced = herald(
        &setup,
        &world,
        &["announce", "--event", &event, "--app", "ci", "--dry-run"],
    );
    assert_eq!(announced.code, DONE, "{}", announced.err);
    assert!(
        announced
            .out
            .contains("POST https://discord.com/api/v10/channels/200/messages\n"),
        "{}",
        announced.out
    );
    assert!(!announced.out.contains("channels/300"), "{}", announced.out);
    assert!(!announced.out.contains("channels/100"), "{}", announced.out);
    assert!(
        announced.out.ends_with("\ndeploys: announced (post)\n"),
        "{}",
        announced.out
    );
}

#[test]
fn an_app_s_event_the_config_does_not_let_through_says_why() {
    let setup = setup(&config());
    let world = World::default();
    let other = written(
        &setup,
        "other.json",
        r#"{"id": "7", "project": "other", "title": "Deployed"}"#,
    );
    let broken = written(&setup, "broken.json", "not json");
    let deployed = written(&setup, "deployed.json", DEPLOYED);
    let missing = setup
        .directory
        .path()
        .join("missing.json")
        .display()
        .to_string();
    let cases = [
        (
            other.as_str(),
            "ci",
            "the app \"ci\" may not announce the project \"other\"; add it to [apps.ci] projects\n",
        ),
        (
            deployed.as_str(),
            "nobody",
            "no app in the config is named \"nobody\"; add [apps.nobody]\n",
        ),
    ];
    for (event, app, why) in cases {
        let refused = herald(
            &setup,
            &world,
            &["announce", "--event", event, "--app", app],
        );
        assert_eq!((refused.code, refused.err.as_str()), (STOPPED, why));
    }
    let unread = herald(
        &setup,
        &world,
        &["announce", "--event", &broken, "--app", "ci"],
    );
    assert_eq!(unread.code, STOPPED);
    assert!(
        unread
            .err
            .starts_with("the event is not herald's event JSON: "),
        "{}",
        unread.err
    );
    let unfound = herald(
        &setup,
        &world,
        &["announce", "--event", &missing, "--app", "ci"],
    );
    assert_eq!(unfound.code, STOPPED);
    assert!(
        unfound
            .err
            .starts_with(&format!("{missing} could not be read: ")),
        "{}",
        unfound.err
    );
}

#[test]
fn an_event_needs_its_app_and_takes_the_place_of_a_forge() {
    let setup = setup(&config());
    let world = World::default();
    let event = written(&setup, "deployed.json", DEPLOYED);
    for args in [
        vec!["announce", "--event", event.as_str()],
        vec!["announce", "--app", "ci"],
        vec![
            "announce",
            "--from",
            "github",
            "--event",
            event.as_str(),
            "--app",
            "ci",
        ],
        vec![
            "announce",
            "--release",
            "v1",
            "--event",
            event.as_str(),
            "--app",
            "ci",
        ],
        vec!["announce"],
    ] {
        let refused = herald(&setup, &world, &args);
        assert_eq!(refused.code, MISTAKEN, "{args:?}: {}", refused.err);
    }
}

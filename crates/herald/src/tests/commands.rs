use std::collections::BTreeMap;

use super::world::*;
use crate::*;

#[test]
fn init_prints_a_new_marker_secret_once_and_needs_no_config() {
    let args = ["herald", "--config", "/nonexistent/herald.toml", "init"].map(ToOwned::to_owned);
    let mut out = Vec::new();
    let code = pollster::block_on(crate::run(
        &args,
        &BTreeMap::new(),
        &|_| Err(String::new()),
        &mut out,
        &mut Vec::new(),
    ));
    let out = String::from_utf8(out).unwrap();
    assert_eq!(code, DONE);
    let secret = out
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("HERALD_MARKER_SECRET="))
        .unwrap();
    assert_eq!(secret.len(), 64);
    assert!(out.contains("herald prints it only this once."), "{out}");
    let mut err = Vec::new();
    assert_eq!(init(None, &mut Vec::new(), &mut err), STOPPED);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "the system's random generator failed, so no secret was made\n"
    );
}

fn answering_once() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = listener.accept().unwrap();
        let _ = stream.read(&mut [0; 1024]);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
    });
    address
}

fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn health_asks_the_listen_variable_then_the_config_and_needs_no_config() {
    let world = World::default();
    let mut listening = setup("not toml");
    let address = answering_once();
    listening
        .vars
        .insert("HERALD_LISTEN".to_owned(), address.clone());
    let healthy = herald(&listening, &world, &["health"]);
    assert_eq!(
        (healthy.code, healthy.out.as_str()),
        (
            DONE,
            format!("herald answers at http://{address}/healthz\n").as_str()
        )
    );
    let port = closed_port();
    let configured = setup(&format!(
        "{CONFIG}[server]\nlisten = \"127.0.0.1:{port}\"\n"
    ));
    let down = herald(&configured, &world, &["health", "--timeout", "1000"]);
    assert_eq!(down.code, STOPPED);
    assert!(
        down.err.starts_with(&format!(
            "herald does not answer at http://127.0.0.1:{port}/healthz: "
        )),
        "{}",
        down.err
    );
}

#[test]
fn store_prints_the_kind_of_store_the_config_names() {
    let world = World::default();
    let kept = herald(&setup(CONFIG), &world, &["store"]);
    assert_eq!((kept.code, kept.out.as_str()), (DONE, "git-branch\n"));
    let broken = herald(&setup("not toml"), &world, &["store"]);
    assert_eq!(broken.code, MISTAKEN);
}

#[test]
fn preview_renders_each_route_against_a_sample_or_a_release_event() {
    let setup = setup(CONFIG);
    let world = World::default();
    let sampled = herald(&setup, &world, &["preview"]);
    assert_eq!(
        (sampled.code, sampled.out.as_str()),
        (
            DONE,
            "releases to discord:\n  title: 📦 sample-project v1.2.0\n  part 1:\n    ## Added\n    \n    - a sample change\n"
        )
    );
    let path = setup
        .directory
        .path()
        .join("event.json")
        .display()
        .to_string();
    let evented = herald(
        &setup,
        &world,
        &["preview", "--event", &path, "--route", "releases"],
    );
    assert!(
        evented
            .out
            .starts_with("releases to discord:\n  title: 📦 cli v2.103.3\n"),
        "{}",
        evented.out
    );
    let threaded = super::world::setup(&CONFIG.replace(
        "template = \"release\"\n",
        "template = \"release\"\nthread = { name = \"{{ event.version }}\" }\n",
    ));
    assert!(
        herald(&threaded, &world, &["preview"])
            .out
            .contains("\n  thread: v1.2.0\n")
    );
    let unknown = herald(&setup, &world, &["preview", "--route", "nope"]);
    assert_eq!(
        (unknown.code, unknown.err.as_str()),
        (MISTAKEN, "no route has the id \"nope\"\n")
    );
}

#[test]
fn preview_says_why_an_event_file_or_a_template_fails() {
    let world = World::default();
    let failing = setup(&CONFIG.replace(
        "title = \"📦 {{ event.project }} {{ event.version }}\"",
        "title = \"{% if event.project == 'cli' %}{{ nothing.here }}{% endif %}📦\"",
    ));
    let path = failing
        .directory
        .path()
        .join("event.json")
        .display()
        .to_string();
    let refused = herald(&failing, &world, &["preview", "--event", &path]);
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .out
            .starts_with("releases to discord: the templates cannot render this event:"),
        "{}",
        refused.out
    );
    let setup = setup(CONFIG);
    let missing = herald(
        &setup,
        &world,
        &["preview", "--event", "/nonexistent/event.json"],
    );
    assert_eq!(missing.code, MISTAKEN);
    assert!(
        missing
            .err
            .starts_with("/nonexistent/event.json could not be read")
    );
    std::fs::write(
        setup.directory.path().join("draft.json"),
        EVENT.replace("\"published\"", "\"created\""),
    )
    .unwrap();
    let draft = setup
        .directory
        .path()
        .join("draft.json")
        .display()
        .to_string();
    let unpublished = herald(&setup, &world, &["preview", "--event", &draft]);
    assert_eq!(
        (unpublished.code, unpublished.err),
        (MISTAKEN, format!("{draft} publishes no release\n"))
    );
    std::fs::write(setup.directory.path().join("garbled.json"), "{}").unwrap();
    let garbled = setup
        .directory
        .path()
        .join("garbled.json")
        .display()
        .to_string();
    assert_eq!(
        herald(&setup, &world, &["preview", "--event", &garbled]).code,
        MISTAKEN
    );
}

#[test]
fn a_dry_run_prints_every_request_redacted_and_sends_and_keeps_nothing() {
    let setup = setup(CONFIG);
    let world = World::default();
    let dry = herald(
        &setup,
        &world,
        &["announce", "--from", "github", "--dry-run"],
    );
    assert_eq!(dry.code, DONE, "{}", dry.err);
    assert!(
        dry.out.starts_with("A dry run sends nothing and keeps no record: these are the requests a first announcement sends.\n\nPOST https://discord.com/api/v10/channels/1566200000000000100/messages\n"),
        "{}",
        dry.out
    );
    assert!(
        dry.out
            .contains("Authorization: Bot «secret:HERALD_DISCORD_TOKEN»")
    );
    assert!(!dry.out.contains("discord-token"));
    assert!(
        dry.out.ends_with("\nreleases: announced (post)\n"),
        "{}",
        dry.out
    );
    assert_eq!(world.state.lock().unwrap().seen, Vec::<String>::new());
    let mut unset = setup;
    unset.vars.remove("HERALD_DISCORD_TOKEN");
    let stopped = herald(
        &unset,
        &world,
        &["announce", "--from", "github", "--dry-run"],
    );
    assert_eq!(
        (stopped.code, stopped.err.as_str()),
        (
            STOPPED,
            "no secret is named HERALD_DISCORD_TOKEN; set it where this platform keeps secrets\n"
        )
    );
}

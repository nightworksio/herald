use std::collections::BTreeMap;

use super::world::*;
use crate::*;

#[test]
fn token_marker_prints_a_new_marker_secret_once_and_needs_no_config() {
    let args = [
        "herald",
        "--config",
        "/nonexistent/herald.toml",
        "token",
        "marker",
    ]
    .map(ToOwned::to_owned);
    let mut out = Vec::new();
    let code = pollster::block_on(crate::run(
        &args,
        &BTreeMap::new(),
        &Connector::new(&|_, _| Err(String::new()), &|_, _| Err(String::new())),
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
    assert_eq!(marker(None, &mut Vec::new(), &mut err), STOPPED);
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
            "destinations.discord.token: no secret is named HERALD_DISCORD_TOKEN; set it where this platform keeps secrets\n"
        )
    );
}

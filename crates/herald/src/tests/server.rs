use std::cell::RefCell;

use herald_core::{RouteId, Step};
use herald_flows::{Announced, FollowError, Followed, Outcome, Stop, Worked};
use herald_server::Heard;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::world::*;
use crate::serving::said;
use crate::*;

fn free() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// What the server on `port` answers a health check, kept in `answer`.
async fn checked(port: u16, answer: &RefCell<String>) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut read = String::new();
    stream.read_to_string(&mut read).await.unwrap();
    *answer.borrow_mut() = read;
}

#[test]
fn the_server_listens_where_it_is_told_until_it_is_stopped() {
    let port = free();
    let answer = RefCell::new(String::new());
    let listen = format!("127.0.0.1:{port}");
    let served = herald_until(
        &setup(CONFIG),
        &World::default(),
        &["server", "--listen", &listen],
        checked(port, &answer),
    );
    assert_eq!(served.code, DONE, "{}", served.err);
    assert!(
        served.out.contains(&format!("herald serves on {listen}")),
        "{}",
        served.out
    );
    assert!(answer.borrow().starts_with("HTTP/1.1 200 OK"));
}

#[test]
fn the_server_listens_where_its_environment_or_its_config_says() {
    let port = free();
    let answer = RefCell::new(String::new());
    let mut told = setup(CONFIG);
    told.vars
        .insert("HERALD_LISTEN".to_owned(), format!("127.0.0.1:{port}"));
    let served = herald_until(
        &told,
        &World::default(),
        &["server"],
        checked(port, &answer),
    );
    assert_eq!(served.code, DONE, "{}", served.err);
    assert!(answer.borrow().starts_with("HTTP/1.1 200 OK"));
    let port = free();
    let answer = RefCell::new(String::new());
    let configured = format!(
        "{CONFIG}[server]\nlisten = \"127.0.0.1:{port}\"\n[server.keys]\ncache = \"store\"\n"
    );
    let served = herald_until(
        &setup(&configured),
        &World::default(),
        &["server"],
        checked(port, &answer),
    );
    assert_eq!(served.code, DONE, "{}", served.err);
    assert!(answer.borrow().starts_with("HTTP/1.1 200 OK"));
}

#[test]
fn a_server_that_cannot_listen_or_read_a_webhook_secret_stops_before_it_serves() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = taken.local_addr().unwrap().to_string();
    let refused = herald_until(
        &setup(CONFIG),
        &World::default(),
        &["server", "--listen", &listen],
        std::future::ready(()),
    );
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .err
            .contains(&format!("herald could not listen on {listen}")),
        "{}",
        refused.err
    );
    let unset = CONFIG.replace(
        "repository = \"cli/cli\"",
        "repository = \"cli/cli\"\nwebhook = { secret = \"HERALD_UNSET_WEBHOOK\" }",
    );
    let unkeyed = herald_until(
        &setup(&unset),
        &World::default(),
        &["server", "--listen", "127.0.0.1:0"],
        std::future::ready(()),
    );
    assert_eq!(unkeyed.code, STOPPED);
    assert!(
        unkeyed.err.contains("HERALD_UNSET_WEBHOOK"),
        "{}",
        unkeyed.err
    );
}

#[test]
fn doctor_names_the_stores_a_server_cannot_keep_records_in() {
    let cached = CONFIG.replace("kind = \"git-branch\"", "kind = \"actions-cache\"");
    let doctored = herald(
        &setup(&cached),
        &World::default(),
        &["doctor", "--layer", "server"],
    );
    assert_eq!(doctored.code, STOPPED);
    assert!(
        doctored.out.contains("store.kind: keeping records in actions-cache cannot be served: this herald keeps records in sqlite, postgres, git-branch, markers or repo-variables; set [store] kind to one of them."),
        "{}",
        doctored.out
    );
}

#[test]
fn the_server_says_what_it_heard_in_a_line_each() {
    let address = "127.0.0.1:8080".parse().unwrap();
    assert_eq!(
        said(Heard::Listening(address)),
        ["herald serves on 127.0.0.1:8080"]
    );
    let announced = Announced {
        route: RouteId::new("releases"),
        outcome: Outcome::Sent {
            steps: vec![Step::Post],
        },
    };
    assert_eq!(
        said(Heard::Worked(Worked::Announced(vec![announced]))),
        ["releases: announced (post)"]
    );
    assert_eq!(said(Heard::Polled(Vec::new())), [] as [String; 0]);
    assert_eq!(said(Heard::Kept(Ok(Vec::new()))), [] as [String; 0]);
    assert_eq!(
        said(Heard::Kept(Err(Stop::Refused("no".to_owned())))),
        ["housekeeping stopped: no. The next round tries again."]
    );
    assert_eq!(
        said(Heard::Unaccepted("too many open files".to_owned())),
        ["a connection could not be taken: too many open files"]
    );
    let followed = |followed| said(Heard::Worked(Worked::Followed(followed)));
    assert_eq!(
        followed(Ok(Followed::Accepted {
            follower: "https://social.example/users/ana".to_owned(),
            actor: "gate".to_owned(),
            answered: herald_core::Sent::Refused("403".to_owned()),
        })),
        ["gate: https://social.example/users/ana follows it now"]
    );
    assert_eq!(
        followed(Ok(Followed::Undone {
            follower: "https://social.example/users/ana".to_owned(),
            actor: "gate".to_owned(),
        })),
        ["gate: https://social.example/users/ana no longer follows it"]
    );
    assert_eq!(
        followed(Ok(Followed::Ignored("it names no signer".to_owned()))),
        ["an activity was ignored: it names no signer"]
    );
    assert_eq!(
        followed(Err(FollowError::Store("down".to_owned()))),
        [
            "an activity was not taken, and may be delivered again: the follower could not be kept: down"
        ]
    );
}

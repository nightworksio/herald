use std::cell::RefCell;

use herald_core::{Asked, Command, DestinationName, Place, RouteId, Step};
use herald_flows::{
    Announced, Answer, Did, FollowError, Followed, Interacted, Job, Outcome, Stop, Worked,
};
use herald_server::Heard;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::world::*;
use crate::serving::said;
use crate::*;

fn worked(did: Did) -> Heard {
    Heard::Worked(Worked {
        did,
        unreleased: None,
    })
}

fn free() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The tries a server gets at a free port, where another test takes the port
/// between its choice and the server's listening on it.
const TRIES: usize = 5;

/// `served` on a free port, and the port: another free one is tried where
/// the one chosen was taken before the server listened on it.
fn on_free_port(served: impl Fn(u16) -> Run) -> (u16, Run) {
    let mut tried = 0;
    loop {
        let port = free();
        let run = served(port);
        tried += 1;
        let taken = run.err.contains("could not listen on") && run.err.contains("in use");
        if !taken || tried == TRIES {
            return (port, run);
        }
    }
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

#[cfg(unix)]
#[test]
fn a_process_stops_when_it_is_asked_to() {
    on_tokio(async {
        let asked = crate::stopped();
        let told = std::process::Command::new("kill")
            .args(["-TERM", &std::process::id().to_string()])
            .status()
            .unwrap();
        assert!(told.success());
        asked.await;
    });
}

#[cfg(unix)]
#[test]
fn a_process_that_cannot_listen_for_its_signals_stops_at_once() {
    let unheard = || Err(std::io::Error::other("no signals here"));
    on_tokio(crate::signalled(unheard(), unheard()));
}

#[test]
fn a_server_refuses_a_store_only_a_run_in_actions_reaches() {
    let cached = CONFIG.replace("kind = \"git-branch\"", "kind = \"actions-cache\"");
    let run = herald(&setup(&cached), &World::default(), &["server"]);
    assert_eq!(run.code, STOPPED);
    assert!(
        run.err.contains("this herald keeps records in"),
        "{}",
        run.err
    );
}

#[test]
fn the_server_listens_where_it_is_told_until_it_is_stopped() {
    let answer = RefCell::new(String::new());
    let (port, served) = on_free_port(|port| {
        herald_until(
            &setup(CONFIG),
            &World::default(),
            &["server", "--listen", &format!("127.0.0.1:{port}")],
            checked(port, &answer),
        )
    });
    let listen = format!("127.0.0.1:{port}");
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
    let answer = RefCell::new(String::new());
    let (_, served) = on_free_port(|port| {
        let mut told = setup(CONFIG);
        told.vars
            .insert("HERALD_LISTEN".to_owned(), format!("127.0.0.1:{port}"));
        herald_until(
            &told,
            &World::default(),
            &["server"],
            checked(port, &answer),
        )
    });
    assert_eq!(served.code, DONE, "{}", served.err);
    assert!(answer.borrow().starts_with("HTTP/1.1 200 OK"));
    let answer = RefCell::new(String::new());
    let (_, served) = on_free_port(|port| {
        let configured = format!(
            "{CONFIG}[server]\nlisten = \"127.0.0.1:{port}\"\n[server.keys]\ncache = \"store\"\n"
        );
        herald_until(
            &setup(&configured),
            &World::default(),
            &["server"],
            checked(port, &answer),
        )
    });
    assert_eq!(served.code, DONE, "{}", served.err);
    assert!(answer.borrow().starts_with("HTTP/1.1 200 OK"));
}

/// What the server on `port` answers an Azure timer's invocation at `path`,
/// kept in `answer`.
async fn timed(port: u16, path: &str, answer: &RefCell<String>) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let request = format!("POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{{}}");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut read = String::new();
    stream.read_to_string(&mut read).await.unwrap();
    *answer.borrow_mut() = read;
}

#[test]
fn under_azure_functions_the_server_listens_on_its_port_and_its_timers_ask_for_rounds() {
    let answer = RefCell::new(String::new());
    let (port, served) = on_free_port(|port| {
        let mut azure = setup(CONFIG);
        azure
            .vars
            .insert("FUNCTIONS_CUSTOMHANDLER_PORT".to_owned(), port.to_string());
        herald_until(
            &azure,
            &World::default(),
            &["server"],
            timed(port, "/housekeep", &answer),
        )
    });
    assert_eq!(served.code, DONE, "{}", served.err);
    assert!(
        served
            .out
            .contains(&format!("herald serves on 127.0.0.1:{port}")),
        "{}",
        served.out
    );
    let answer = answer.borrow();
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(
        answer.ends_with(r#"{"Outputs":{},"Logs":[],"ReturnValue":null}"#),
        "{answer}"
    );
}

/// What the server on `port` answers a request for a housekeeping round
/// carrying `secret` as its bearer token, kept in `answer`.
async fn rounded(port: u16, secret: &str, answer: &RefCell<String>) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let request = format!(
        "POST /herald/round/housekeep HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {secret}\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut read = String::new();
    stream.read_to_string(&mut read).await.unwrap();
    *answer.borrow_mut() = read;
}

/// What the server answers a round asked with `given` where its
/// `HERALD_ROUND_SECRET` is `set`.
fn round_answered(set: &str, given: &str) -> String {
    let answer = RefCell::new(String::new());
    let (port, served) = on_free_port(|port| {
        let mut told = setup(CONFIG);
        told.vars
            .insert(herald_flows::ROUND_SECRET.to_owned(), set.to_owned());
        herald_until(
            &told,
            &World::default(),
            &["server", "--listen", &format!("127.0.0.1:{port}")],
            rounded(port, given, &answer),
        )
    });
    assert_eq!(served.code, DONE, "{} on {port}", served.err);
    answer.into_inner()
}

#[test]
fn a_round_runs_when_asked_with_the_round_secret_and_an_empty_one_is_none() {
    let ran = round_answered("s3cret", "s3cret");
    assert!(ran.starts_with("HTTP/1.1 200 OK"), "{ran}");
    assert!(ran.ends_with("the round ran"), "{ran}");
    let unset = round_answered("", "");
    assert!(unset.starts_with("HTTP/1.1 401"), "{unset}");
}

#[test]
fn a_server_that_cannot_listen_or_read_a_webhook_or_command_secret_stops_before_it_serves() {
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
    let unsigned = format!(
        "{CONFIG}[destinations.slack]\nkind = \"slack\"\ntoken = {{ secret = \"HERALD_DISCORD_TOKEN\" }}\n[destinations.slack.interactions]\nsigning = {{ secret = \"HERALD_UNSET_SIGNING\" }}\n"
    );
    let unanswering = herald_until(
        &setup(&unsigned),
        &World::default(),
        &["server", "--listen", "127.0.0.1:0"],
        std::future::ready(()),
    );
    assert_eq!(unanswering.code, STOPPED);
    assert!(
        unanswering.err.contains("HERALD_UNSET_SIGNING"),
        "{}",
        unanswering.err
    );
}

#[test]
fn the_server_warns_of_what_a_server_cannot_serve_before_it_starts() {
    let cached = CONFIG.replace("kind = \"git-branch\"", "kind = \"actions-cache\"");
    let refused = herald_until(
        &setup(&cached),
        &World::default(),
        &["server", "--listen", "127.0.0.1:0"],
        std::future::ready(()),
    );
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .out
            .contains("keeping records in actions-cache cannot be served"),
        "{}",
        refused.out
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
        doctored.out.contains("store.kind: keeping records in actions-cache cannot be served: this herald keeps records in sqlite, postgres, dynamodb, git-branch, markers or repo-variables; set [store] kind to one of them."),
        "{}",
        doctored.out
    );
}

#[test]
fn the_server_says_how_each_command_was_answered_and_what_its_sessions_said() {
    let asked = Asked {
        destination: DestinationName::new("discord"),
        place: Place::new("323456789012345678"),
        user: "423456789012345678".to_owned(),
        command: Command::Latest,
        project: None,
        share: false,
        toggles: false,
        member: None,
    };
    let interacted = |trouble: Option<&str>, sent: Result<(), String>| {
        said(worked(Did::Interacted(vec![Interacted {
            asked: asked.clone(),
            answer: Answer {
                said: String::new(),
                public: false,
                trouble: trouble.map(ToOwned::to_owned),
            },
            sent,
        }])))
    };
    assert_eq!(
        interacted(None, Ok(())),
        ["discord: latest was answered in 323456789012345678"]
    );
    assert_eq!(
        interacted(None, Err("refused".to_owned())),
        ["discord: the answer to latest could not be sent: refused"]
    );
    assert_eq!(
        interacted(Some("the store is down"), Ok(())),
        ["discord: latest could not be answered: the store is down"]
    );
    assert_eq!(
        said(Heard::Session {
            destination: DestinationName::new("slack"),
            said: "Slack's Socket Mode could not be opened: invalid_auth".to_owned(),
        }),
        ["slack: Slack's Socket Mode could not be opened: invalid_auth"]
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
        said(worked(Did::Announced(vec![announced]))),
        ["releases: announced (post)"]
    );
    assert_eq!(said(Heard::Polled(Vec::new())), [] as [String; 0]);
    assert_eq!(
        said(Heard::Kept(Ok(herald_flows::Housekept::default()))),
        [] as [String; 0]
    );
    assert_eq!(
        said(Heard::Kept(Err(Stop::Refused("no".to_owned())))),
        ["housekeeping stopped: no. The next round tries again."]
    );
    assert_eq!(
        said(Heard::Unworked(
            Box::new(Job::Interact {
                destination: DestinationName::new("chat"),
                asked: Vec::new(),
            }),
            Ok(())
        )),
        ["herald stopped before it worked the commands given on chat"]
    );
    assert_eq!(
        said(Heard::Unaccepted("too many open files".to_owned())),
        ["a connection could not be taken: too many open files"]
    );
    assert_eq!(
        said(Heard::Refused("the signature does not match".to_owned())),
        ["a request was refused: the signature does not match"]
    );
    let followed = |followed| said(worked(Did::Followed(followed)));
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

#[test]
fn azure_s_route_prefix_is_its_app_setting_else_host_json_else_api() {
    use crate::serving::{azure_prefix, timers};

    let set = |value: &str| {
        BTreeMap::from([(
            "AzureFunctionsJobHost__extensions__http__routePrefix".to_owned(),
            value.to_owned(),
        )])
    };
    let host = r#"{"extensions": {"http": {"routePrefix": "/hooks/"}}}"#;
    assert_eq!(azure_prefix(&set("/edge"), Some(host)), "edge");
    assert_eq!(azure_prefix(&set(""), Some(host)), "");
    assert_eq!(azure_prefix(&BTreeMap::new(), Some(host)), "hooks");
    assert_eq!(azure_prefix(&BTreeMap::new(), Some("{}")), "api");
    assert_eq!(azure_prefix(&BTreeMap::new(), Some("not json")), "api");
    assert_eq!(azure_prefix(&BTreeMap::new(), None), "api");
    assert_eq!(timers("api").map(|asked| asked.prefix), Some("api"));
    assert!(timers("").is_none());
}

use herald_compose::{EGRESS_GUARDED, Host as _};
use herald_ports::{Http as _, HttpError, Method, PUBLIC, Request, Text};

use super::host::WasiHost;

fn hosted() -> WasiHost {
    WasiHost::new([
        ("HERALD_TIMEOUT_MS".to_owned(), "50".to_owned()),
        ("GITHUB_REPOSITORY".to_owned(), "cli/cli".to_owned()),
        ("EMPTY".to_owned(), String::new()),
    ])
}

#[test]
fn the_host_gives_its_variables_and_refuses_a_database() {
    let host = hosted();
    assert_eq!(host.var("GITHUB_REPOSITORY"), Some("cli/cli"));
    assert_eq!(host.var("EMPTY"), None);
    assert_eq!(host.var("UNSET"), None);
    assert!(wstd::runtime::block_on(host.kv(herald_compose::KvAt::DenoKv(None))).is_err());
}

#[test]
fn a_connection_refuses_what_it_may_not_reach() {
    let http = hosted().connect("https://discord.com/api").unwrap();
    let elsewhere = Request::new(Method::Get, Text::plain("https://example.org/"));
    assert!(wstd::runtime::block_on(http.send(elsewhere)).is_err());
    assert!(hosted().connect("not a url").is_err());
}

#[test]
fn a_public_connection_refuses_a_name_until_the_host_is_said_to_be_guarded() {
    let asked = || Request::new(Method::Get, Text::plain("https://localhost:9/"));
    let http = hosted().connect(PUBLIC).unwrap();
    assert!(matches!(
        wstd::runtime::block_on(http.send(asked())),
        Err(HttpError::Refused { .. })
    ));
    if std::env::var("HERALD_TEST_BASE").is_err() {
        return;
    }
    let guarded = WasiHost::new([(EGRESS_GUARDED.to_owned(), "true".to_owned())]);
    let http = guarded.connect(PUBLIC).unwrap();
    assert!(!matches!(
        wstd::runtime::block_on(http.send(asked())),
        Err(HttpError::Refused { .. })
    ));
}

#[test]
fn a_connection_fetches_what_it_may_reach_and_reads_no_further_than_asked() {
    let Ok(base) = std::env::var("HERALD_TEST_BASE") else {
        return;
    };
    let http = hosted().connect(&base).unwrap();
    let mut asked = Request::new(Method::Post, Text::plain(format!("{base}/said")));
    asked.body = b"hello".to_vec();
    let answer = wstd::runtime::block_on(http.send(asked)).unwrap();
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, b"POST /said hello");
    assert!(
        answer
            .headers
            .iter()
            .any(|(name, value)| name == "x-fixture" && value == "yes")
    );
    let mut little = Request::new(Method::Get, Text::plain(format!("{base}/said")));
    little.most = 2;
    assert!(wstd::runtime::block_on(http.send(little)).is_err());
}

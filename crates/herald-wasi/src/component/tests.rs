use herald_compose::Host as _;
use herald_ports::{Http as _, Method, Request, Text};

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

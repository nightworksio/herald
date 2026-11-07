use herald_compose::Host as _;
use herald_ports::{Http as _, Method, Request, Text};

use super::host::FastlyHost;

#[test]
fn a_connection_refuses_what_it_may_not_reach() {
    let host = FastlyHost::new();
    let http = host.connect("https://discord.com/api").unwrap();
    let elsewhere = Request::new(Method::Get, Text::plain("https://example.org/"));
    assert!(pollster::block_on(http.send(elsewhere)).is_err());
    assert!(host.connect("not a url").is_err());
}

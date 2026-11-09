use herald_core::DestinationName;
use herald_ports::Response;

use super::*;

/// A session that names how it connects and keeps every other default.
struct Plain;

impl Session for Plain {
    fn connect(&mut self) -> Connect {
        Connect::Socket("wss://plain.example".to_owned())
    }

    fn unreachable(&self) -> &'static str {
        "plain is unreachable"
    }
}

#[test]
fn a_session_hears_nothing_and_connects_again_when_closed_unless_it_says_otherwise() {
    let mut plain = Plain;
    let destination = DestinationName::new("plain");
    let response = Response {
        status: 200,
        headers: Vec::new(),
        body: b"{}".to_vec(),
    };

    assert_eq!(
        plain.connect(),
        Connect::Socket("wss://plain.example".to_owned())
    );
    assert_eq!(
        plain.opened(&response),
        Err("it opens no socket by a request".to_owned())
    );
    assert_eq!(plain.heard(&destination, "{}"), []);
    assert_eq!(plain.closed(Some(1000)), [Effect::Reconnect]);
    assert_eq!(plain.closed(None), [Effect::Reconnect]);
    assert_eq!(plain.beat(), []);
    assert_eq!(plain.polled(&destination, &response), Ok(Vec::new()));
    assert_eq!(plain.unreachable(), "plain is unreachable");
}

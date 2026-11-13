//! Whatever text arrives as a URL, reading it gives one host that its own
//! origin reads back to, a socket is taken only on a host under its domains
//! with no user before it, and escaping a text reads back to that text.

#![no_main]

use herald_core::{Address, Escaping, socket_under, under_domain, web_url};
use libfuzzer_sys::fuzz_target;

const DOMAINS: [&str; 2] = ["slack.com", "discord.gg"];

fuzz_target!(|text: &str| {
    if let Some(address) = Address::parse(text) {
        assert!(!address.host().is_empty());
        assert!(address.authority().starts_with(address.host()));
        let origin = Address::parse(&address.origin()).expect("an origin reads back");
        assert_eq!(origin.host(), address.host());
        assert_eq!(origin.authority(), address.authority());
        assert!(!origin.names_a_user());
    }
    if socket_under(text, &DOMAINS) {
        let address = Address::parse(text).expect("a socket taken is an address");
        assert_eq!(address.scheme(), "wss");
        assert!(!address.names_a_user());
        assert!(DOMAINS.iter().any(|domain| under_domain(address.host(), domain)));
    }
    if web_url(text) {
        assert!(Address::parse(text).is_some());
    }
    let escaped = Escaping::UNRESERVED.escape(text);
    assert!(escaped.bytes().all(|byte| (Escaping::UNRESERVED.keeps)(byte) || byte == b'%' || byte.is_ascii_hexdigit()));
    assert_eq!(Escaping::UNRESERVED.unescape(&escaped).as_deref(), Some(text));
});

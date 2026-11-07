use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Whether `url` may be reached on others' behalf: it is on `scheme`, names
/// no user, and its host is a name, or an address `admits` (ADR-0007).
#[must_use]
pub fn reachable(url: &Url, scheme: &str, admits: fn(IpAddr) -> bool) -> bool {
    url.scheme() == scheme
        && url.username().is_empty()
        && url.password().is_none()
        && match url.host() {
            Some(Host::Domain(_)) => true,
            Some(Host::Ipv4(address)) => admits(IpAddr::V4(address)),
            Some(Host::Ipv6(address)) => admits(IpAddr::V6(address)),
            None => false,
        }
}

/// Whether `address` is on the public internet. An IPv4 address is not
/// loopback, private, shared, link-local, documentation, benchmarking,
/// reserved, multicast or unspecified. An IPv6 address is a global unicast
/// one, in `2000::/3`, outside its IETF protocol assignments and
/// documentation ranges, or carries such an IPv4 address.
#[must_use]
pub fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => public_v6(address),
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_documentation()
        || address.is_multicast()
        || first == 0
        || first >= 240
        || (first == 100 && (64..=127).contains(&second))
        || (first == 192 && second == 0 && third == 0)
        || (first == 198 && (second == 18 || second == 19)))
}

fn public_v6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let carried = |high: u16, low: u16| {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        public_v4(Ipv4Addr::new(a, b, c, d))
    };
    match segments {
        [0, 0, 0, 0, 0, 0xffff, high, low]
        | [0, 0, 0, 0, 0xffff, 0, high, low]
        | [0x64, 0xff9b, 0, 0, 0, 0, high, low]
        | [0x2002, high, low, ..] => carried(high, low),
        [0x2001, 0..=0x1ff | 0xdb8, ..] => false,
        [first, ..] => first & 0xe000 == 0x2000 && first & 0xfff0 != 0x3ff0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_addresses_on_the_public_internet_are_public() {
        for address in [
            "93.184.216.34",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "192.0.1.1",
            "198.17.0.1",
            "198.20.0.1",
            "2606:4700::1111",
            "::ffff:93.184.216.34",
            "::ffff:0:5db8:d822",
            "2001:200::1",
            "3fef::1",
            "64:ff9b::5db8:d822",
            "2002:5db8:d822::1",
        ] {
            assert!(public(address.parse().unwrap()), "{address}");
        }
        for address in [
            "0.0.0.0",
            "0.1.2.3",
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "255.255.255.255",
            "192.0.2.1",
            "224.0.0.1",
            "240.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "192.0.0.8",
            "198.18.0.1",
            "198.19.255.255",
            "::",
            "::1",
            "::127.0.0.1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "2002:a00:1::1",
            "100::1",
            "2001::1",
            "2001:2::1",
            "2001:10::1",
            "2001:1ff::1",
            "::ffff:0:a00:1",
            "::ffff:0:7f00:1",
            "2001:db8::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "3fff::1",
            "3ffe::1",
            "3fff:fff::1",
            "5f00::1",
            "100:1::1",
            "4000::1",
            "1fff::1",
            "c000::1",
            "feff::1",
            "64:ff9b:1::5db8:d822",
            "ff02::1",
        ] {
            assert!(!public(address.parse().unwrap()), "{address}");
        }
    }

    #[test]
    fn a_url_is_reachable_on_its_scheme_with_no_user_at_a_name_or_an_admitted_address() {
        let url = |text: &str| Url::parse(text).unwrap();
        for text in [
            "https://social.example/x",
            "https://1.1.1.1/",
            "https://[2606:4700::1111]/",
        ] {
            assert!(reachable(&url(text), "https", public), "{text}");
        }
        for text in [
            "http://social.example/x",
            "https://ana@social.example/",
            "https://:pw@social.example/",
            "https://10.0.0.1/",
            "https://[::1]/",
            "data:text/plain,x",
        ] {
            assert!(!reachable(&url(text), "https", public), "{text}");
        }
    }
}

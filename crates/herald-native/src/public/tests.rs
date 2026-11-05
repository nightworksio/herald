use herald_ports::{Method, Text};

use super::*;
use crate::serving::{answer, serve};

fn admitting(scheme: &'static str, admits: fn(IpAddr) -> bool) -> PublicHttp {
    let tls = tls(rustls::crypto::ring::default_provider()).unwrap();
    PublicHttp::built(admits, scheme, Some(tls), CONNECT, Some(TOTAL)).unwrap()
}

fn get(url: &str) -> Request {
    Request::new(Method::Get, Text::plain(url))
}

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

#[tokio::test]
async fn a_url_off_https_with_a_user_or_on_a_private_address_is_refused_unsent() {
    let http = PublicHttp::new().unwrap();
    for url in [
        "http://social.example/users/ana",
        "https://ana@social.example/users/ana",
        "https://ana:pw@social.example/users/ana",
        "https://:pw@social.example/users/ana",
        "https://127.0.0.1/users/ana",
        "https://[::1]/users/ana",
        "https://[fd00::1]/users/ana",
        "https://10.0.0.1/users/ana",
        "file:///etc/passwd",
        "not a url",
    ] {
        assert_eq!(
            http.send(get(url)).await,
            Err(HttpError::Refused {
                url: url.to_owned()
            }),
            "{url}"
        );
    }
}

#[tokio::test]
async fn a_request_hung_up_before_its_body_is_complete_is_kept_as_read() {
    use tokio::io::AsyncWriteExt;
    let (base, server) = serve(vec![answer("200 OK", "", b"")]).await;
    let mut stream = tokio::net::TcpStream::connect(base.trim_start_matches("http://"))
        .await
        .unwrap();
    stream.write_all(b"POST /inbox HTTP/1.1\r\n").await.unwrap();
    stream.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    stream
        .write_all(b"Content-Length: 10\r\n\r\nabc")
        .await
        .unwrap();
    drop(stream);
    let requests = server.await.unwrap();
    assert!(
        requests[0].starts_with("POST /inbox") && requests[0].ends_with("\r\n\r\nabc"),
        "{}",
        requests[0]
    );
}

#[tokio::test]
async fn a_url_without_a_host_is_refused_unsent() {
    let url = "data:text/plain,herald";
    assert_eq!(
        admitting("data", |_| true).send(get(url)).await,
        Err(HttpError::Refused {
            url: url.to_owned()
        })
    );
}

#[tokio::test]
async fn a_host_is_reached_only_on_the_addresses_it_resolves_to_and_admits() {
    let (base, server) = serve(vec![answer("200 OK", "", b"{\"id\":\"ana\"}")]).await;
    let port = base.rsplit(':').next().unwrap();
    let named = format!("http://localhost:{port}/users/ana");
    let refused = admitting("http", public).send(get(&named)).await;
    let Err(HttpError::Unreachable { reason }) = refused else {
        panic!("{refused:?}");
    };
    assert!(
        reason.contains("localhost resolves to an address that is not public"),
        "{reason}"
    );
    let response = admitting("http", |_| true).send(get(&named)).await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"{\"id\":\"ana\"}");
    let requests = server.await.unwrap();
    assert!(
        requests[0].starts_with("GET /users/ana HTTP/1.1"),
        "{}",
        requests[0]
    );
}

#[test]
fn a_host_that_resolves_to_nothing_is_refused() {
    let Err(refusal) = vetted("herald.invalid".to_owned(), Vec::new(), |_| true) else {
        panic!("resolved to nothing and was admitted");
    };
    assert_eq!(
        refusal.to_string(),
        "herald.invalid resolves to an address that is not public"
    );
}

#[test]
fn a_host_is_admitted_only_where_it_resolves_and_every_address_is_public() {
    let at = |address: &str| SocketAddr::new(address.parse().unwrap(), 443);
    assert!(admitted(
        &[at("93.184.216.34"), at("2606:4700::1111")],
        public
    ));
    assert!(!admitted(&[at("93.184.216.34"), at("10.0.0.1")], public));
    assert!(!admitted(&[at("10.0.0.1"), at("93.184.216.34")], public));
    assert!(!admitted(&[], public));
}

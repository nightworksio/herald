use super::*;
use tokio::net::TcpListener;

use crate::serving::{answer, serve};
use herald_ports::{Secret, Text};

fn http(base: &str) -> NativeHttp {
    NativeHttp::new(Reach::new([base]).unwrap(), &Outbound::default()).unwrap()
}

#[tokio::test]
async fn a_request_is_sent_as_given_and_its_response_read_back() {
    let (base, server) = serve(vec![answer(
        "201 Created",
        "X-Test: yes\r\n",
        b"{\"id\":\"1\"}",
    )])
    .await;
    let request = Request::new(
        Method::Post,
        Text::plain(format!("{base}/channels/1/messages")),
    )
    .header(
        "Authorization",
        Text::plain("Bot ").then_secret(Secret::new("TOKEN", "hunter2")),
    )
    .header("Content-Type", Text::plain("application/json"))
    .body(r#"{"content":"hi"}"#);
    let response = http(&base).send(request).await.unwrap();
    assert_eq!(response.status, 201);
    assert_eq!(response.header("x-test"), Some("yes"));
    assert_eq!(response.body, b"{\"id\":\"1\"}");
    let sent = server.await.unwrap();
    assert!(
        sent[0].starts_with("POST /channels/1/messages HTTP/1.1\r\n"),
        "{}",
        sent[0]
    );
    assert!(
        sent[0].contains("authorization: Bot hunter2\r\n"),
        "{}",
        sent[0]
    );
    assert!(
        sent[0].contains("content-type: application/json\r\n"),
        "{}",
        sent[0]
    );
    assert!(
        sent[0].ends_with("\r\n\r\n{\"content\":\"hi\"}"),
        "{}",
        sent[0]
    );
}

#[tokio::test]
async fn each_method_is_sent_as_http_spells_it() {
    let methods = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
    ];
    let (base, server) = serve(
        methods
            .iter()
            .map(|_| answer("204 No Content", "", b""))
            .collect(),
    )
    .await;
    let http = http(&base);
    for method in methods {
        let response = http
            .send(Request::new(method, Text::plain(format!("{base}/"))))
            .await
            .unwrap();
        assert_eq!(response.status, 204);
    }
    let sent: Vec<String> = server
        .await
        .unwrap()
        .iter()
        .map(|request| request.split(' ').next().unwrap().to_owned())
        .collect();
    assert_eq!(sent, ["GET", "POST", "PUT", "PATCH", "DELETE"]);
}

#[tokio::test]
async fn a_url_outside_the_reach_is_refused_with_its_secrets_redacted() {
    let http = http("https://discord.com/api/v10");
    let url = Text::plain("https://example.org/api/webhooks/1/")
        .then_secret(Secret::new("HERALD_DISCORD_WEBHOOK", "hunter2"));
    assert_eq!(
        http.send(Request::new(Method::Post, url)).await,
        Err(HttpError::Refused {
            url: "https://example.org/api/webhooks/1/«secret:HERALD_DISCORD_WEBHOOK»".to_owned()
        })
    );
    assert_eq!(
        http.send(Request::new(Method::Get, Text::plain("not a url")))
            .await,
        Err(HttpError::Refused {
            url: "not a url".to_owned()
        })
    );
}

#[tokio::test]
async fn a_redirect_is_answered_not_followed() {
    let (base, server) = serve(vec![answer(
        "302 Found",
        "Location: https://example.org/\r\n",
        b"",
    )])
    .await;
    let response = http(&base)
        .send(Request::new(Method::Get, Text::plain(format!("{base}/"))))
        .await
        .unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(response.header("location"), Some("https://example.org/"));
    assert_eq!(server.await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_host_that_cannot_be_reached_says_why_without_its_url() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let url = Text::plain(format!("{base}/")).then_secret(Secret::new("HOOK", "hunter2"));
    let Err(HttpError::Unreachable { reason }) =
        http(&base).send(Request::new(Method::Get, url)).await
    else {
        panic!("reached a closed port");
    };
    assert!(reason.contains("onnect"), "{reason}");
    assert!(!reason.contains("hunter2"), "{reason}");
    assert!(!reason.contains(&base), "{reason}");
}

#[tokio::test]
async fn a_host_that_does_not_answer_in_time_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let silent = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(stream);
    });
    let http = NativeHttp::built(
        Reach::new([base.as_str()]).unwrap(),
        Some(tls(rustls::crypto::ring::default_provider()).unwrap()),
        CONNECT,
        Some(Duration::from_millis(200)),
    )
    .unwrap();
    assert_eq!(
        http.send(Request::new(Method::Get, Text::plain(format!("{base}/"))))
            .await,
        Err(HttpError::TimedOut)
    );
    silent.abort();
}

#[tokio::test]
async fn a_body_up_to_the_cap_is_read_and_a_larger_one_refused() {
    let full = vec![b'a'; Outbound::BODY];
    let over = vec![b'a'; Outbound::BODY + 1];
    let (base, server) = serve(vec![
        answer("200 OK", "", &full),
        answer("200 OK", "", &over),
    ])
    .await;
    let http = http(&base);
    let request = || Request::new(Method::Get, Text::plain(format!("{base}/")));
    assert_eq!(
        http.send(request()).await.unwrap().body.len(),
        Outbound::BODY
    );
    assert_eq!(
        http.send(request()).await,
        Err(HttpError::Unreachable {
            reason: "the response is larger than 33554432 bytes".to_owned()
        })
    );
    server.await.unwrap();
}

#[tokio::test]
async fn a_waiting_client_waits_past_the_usual_bound_and_reaches_its_origin_alone() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let slow = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
            .await
            .unwrap();
    });
    let http = NativeHttp::waiting(&base).unwrap();
    let answered = http
        .send(Request::new(
            Method::Get,
            Text::plain(format!("{base}/next")),
        ))
        .await
        .unwrap();
    assert_eq!(answered.body, b"ok");
    slow.await.unwrap();
    assert!(NativeHttp::waiting("http://10.0.0.1").is_err());
}

#[test]
fn a_client_without_tls_is_not_built() {
    let error = NativeHttp::built(
        Reach::new(["https://discord.com"]).unwrap(),
        None,
        CONNECT,
        Some(duration(Outbound::TOTAL)),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("the HTTP client could not be built: "),
        "{error}"
    );
}

#[test]
fn tls_without_a_cipher_suite_is_not_set_up() {
    let provider = CryptoProvider {
        cipher_suites: Vec::new(),
        ..rustls::crypto::ring::default_provider()
    };
    let error = tls(provider).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("the HTTP client could not be built: "),
        "{error}"
    );
}

#[test]
fn a_client_for_a_base_reaches_only_that_base_or_says_why_not() {
    assert!(NativeHttp::reaching("https://discord.com/api/v10", &Outbound::default()).is_ok());
    let refused = NativeHttp::reaching("http://discord.com", &Outbound::default()).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "http://discord.com is not HTTPS; plain HTTP is allowed only on a loopback host"
    );
    let unbuilt = ConnectError::Build(BuildError("no TLS".to_owned()));
    assert_eq!(
        unbuilt.to_string(),
        "the HTTP client could not be built: no TLS"
    );
}

#[tokio::test]
async fn a_request_reads_at_most_its_own_cap_whether_or_not_the_length_is_said() {
    let unsaid = |body: &[u8]| {
        [
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".as_slice(),
            body,
        ]
        .concat()
    };
    let (base, server) = serve(vec![
        answer("200 OK", "", b"abcd"),
        answer("200 OK", "", b"abcde"),
        unsaid(b"abcd"),
        unsaid(b"abcde"),
    ])
    .await;
    let http = http(&base);
    let request = || Request::new(Method::Get, Text::plain(format!("{base}/"))).at_most(4);
    let larger = Err(HttpError::Unreachable {
        reason: "the response is larger than 4 bytes".to_owned(),
    });
    assert_eq!(http.send(request()).await.unwrap().body, b"abcd");
    assert_eq!(http.send(request()).await, larger);
    assert_eq!(http.send(request()).await.unwrap().body, b"abcd");
    assert_eq!(http.send(request()).await, larger);
    server.await.unwrap();
    assert_eq!(
        Request::new(Method::Get, Text::plain("x"))
            .at_most(Outbound::BODY_MOST + 1)
            .most,
        Outbound::BODY_MOST
    );
}

#[tokio::test]
async fn a_response_is_read_no_further_than_the_config_s_body() {
    let (base, server) = serve(vec![
        answer("200 OK", "", b"abcd"),
        answer("200 OK", "", b"abcde"),
    ])
    .await;
    let outbound = Outbound {
        body: 4,
        ..Outbound::default()
    };
    let http = NativeHttp::new(Reach::new([base.as_str()]).unwrap(), &outbound).unwrap();
    let request = || Request::new(Method::Get, Text::plain(format!("{base}/")));
    assert_eq!(http.send(request()).await.unwrap().body, b"abcd");
    assert_eq!(
        http.send(request()).await,
        Err(HttpError::Unreachable {
            reason: "the response is larger than 4 bytes".to_owned(),
        })
    );
    server.await.unwrap();
}

#[test]
fn a_config_s_roots_are_trusted_beside_the_mozilla_roots() {
    let mozilla = trusted(None).unwrap().len();
    let roots = Roots {
        file: "test-ca.pem".to_owned(),
        pem: include_str!("../../fixtures/test-ca.pem").to_owned(),
    };
    assert_eq!(trusted(Some(&roots)).unwrap().len(), mozilla + 1);
    let broken = Roots {
        file: "broken.pem".to_owned(),
        pem: "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n".to_owned(),
    };
    let refused = trusted(Some(&broken)).unwrap_err();
    assert!(refused.0.starts_with("broken.pem: "), "{refused}");
    let unencoded = Roots {
        file: "unencoded.pem".to_owned(),
        pem: "-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n".to_owned(),
    };
    let unread = trusted(Some(&unencoded)).unwrap_err();
    assert!(unread.0.starts_with("unencoded.pem: "), "{unread}");
    let outbound = Outbound {
        roots: Some(broken),
        ..Outbound::default()
    };
    assert_eq!(
        NativeHttp::new(Reach::new(["https://a.example"]).unwrap(), &outbound).err(),
        Some(refused)
    );
}

#[tokio::test]
async fn a_request_goes_through_the_proxy_a_config_names() {
    let (proxy, server) = serve(vec![answer("200 OK", "", b"proxied")]).await;
    let outbound = Outbound {
        proxy: Proxy::At(proxy),
        ..Outbound::default()
    };
    let http = NativeHttp::new(Reach::new(["http://127.0.0.1:9"]).unwrap(), &outbound).unwrap();
    let answer = http
        .send(Request::new(
            Method::Get,
            Text::plain("http://127.0.0.1:9/path"),
        ))
        .await
        .unwrap();
    assert_eq!(answer.body, b"proxied");
    let asked = server.await.unwrap();
    assert!(
        asked[0].starts_with("GET http://127.0.0.1:9/path HTTP/1.1"),
        "{}",
        asked[0]
    );
}

#[test]
fn a_proxy_that_is_no_url_is_refused() {
    let outbound = Outbound {
        proxy: Proxy::At("http://exa mple:3128".to_owned()),
        ..Outbound::default()
    };
    assert!(NativeHttp::new(Reach::new(["https://a.example"]).unwrap(), &outbound).is_err());
}

#[tokio::test]
async fn a_request_set_to_no_proxy_goes_straight_to_its_origin() {
    let (base, server) = serve(vec![answer("200 OK", "", b"direct")]).await;
    let outbound = Outbound {
        proxy: Proxy::None,
        ..Outbound::default()
    };
    let http = NativeHttp::new(Reach::new([base.as_str()]).unwrap(), &outbound).unwrap();
    let answer = http
        .send(Request::new(
            Method::Get,
            Text::plain(format!("{base}/path")),
        ))
        .await
        .unwrap();
    assert_eq!(answer.body, b"direct");
    let asked = server.await.unwrap();
    assert!(asked[0].starts_with("GET /path HTTP/1.1"), "{}", asked[0]);
}

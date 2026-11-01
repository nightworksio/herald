use herald_ports::{Secret, Text};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::*;

async fn request_of(stream: &mut TcpStream) -> String {
    let mut seen = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await.unwrap();
        seen.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&seen).into_owned();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .map_or(0, |length| length.parse::<usize>().unwrap());
            if seen.len() >= end + 4 + length {
                return text;
            }
        }
        if read == 0 {
            return text;
        }
    }
}

async fn serve(answers: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for answer in answers {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(request_of(&mut stream).await);
            stream.write_all(&answer).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        requests
    });
    (base, server)
}

fn answer(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n",
        body.len()
    );
    [head.as_bytes(), body].concat()
}

fn http(base: &str) -> NativeHttp {
    NativeHttp::new(Reach::new([base]).unwrap()).unwrap()
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
        Duration::from_millis(200),
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
    let full = vec![b'a'; BODY_CAP];
    let over = vec![b'a'; BODY_CAP + 1];
    let (base, server) = serve(vec![
        answer("200 OK", "", &full),
        answer("200 OK", "", &over),
    ])
    .await;
    let http = http(&base);
    let request = || Request::new(Method::Get, Text::plain(format!("{base}/")));
    assert_eq!(http.send(request()).await.unwrap().body.len(), BODY_CAP);
    assert_eq!(
        http.send(request()).await,
        Err(HttpError::Unreachable {
            reason: "the response is larger than 8388608 bytes".to_owned()
        })
    );
    server.await.unwrap();
}

#[test]
fn a_client_without_tls_is_not_built() {
    let error = NativeHttp::built(
        Reach::new(["https://discord.com"]).unwrap(),
        None,
        CONNECT,
        TOTAL,
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

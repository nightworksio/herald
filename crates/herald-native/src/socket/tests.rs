use super::*;

fn sockets() -> NativeSockets {
    NativeSockets::under(["discord.gg", "slack.com"]).unwrap()
}

fn allowed(url: &str) -> bool {
    sockets().allows(&Url::parse(url).unwrap())
}

#[test]
fn only_secure_connections_under_the_domains_and_plain_ones_on_loopback_are_allowed() {
    assert!(allowed("wss://gateway.discord.gg/?v=10"));
    assert!(allowed("wss://gateway-us-east1-b.discord.gg"));
    assert!(allowed("wss://discord.gg"));
    assert!(allowed("wss://wss-primary.slack.com/link/?ticket=1"));
    assert!(!allowed("wss://evildiscord.gg"));
    assert!(!allowed("wss://discord.gg.evil.example"));
    assert!(!allowed("ws://gateway.discord.gg"));
    assert!(!allowed("https://gateway.discord.gg"));
    assert!(allowed("ws://127.0.0.1:9000"));
    assert!(allowed("ws://[::1]:9000"));
    assert!(allowed("ws://localhost:9000"));
    assert!(!allowed("ws://10.0.0.1"));
    assert!(!allowed("wss://127.0.0.1"));
    assert!(!allowed("data:text/plain,discord.gg"));
}

#[test]
fn a_url_not_allowed_is_refused_without_its_query() {
    let refused = pollster::block_on(sockets().open("wss://evil.example/?ticket=secret"));
    assert_eq!(
        refused.err(),
        Some(HttpError::Refused {
            url: "wss://evil.example/".to_owned()
        })
    );
    let unparsed = pollster::block_on(sockets().open("not a url"));
    assert!(matches!(unparsed, Err(HttpError::Refused { .. })));
}

#[tokio::test]
async fn a_connection_carries_text_frames_both_ways_and_says_how_it_closed() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let heard = socket.next().await.unwrap().unwrap();
        socket.send(Message::Ping(Vec::new().into())).await.unwrap();
        socket
            .send(Message::text(format!("echo {}", heard.to_text().unwrap())))
            .await
            .unwrap();
        socket
            .close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: 4004.into(),
                reason: "".into(),
            }))
            .await
            .unwrap();
    });
    let mut socket = sockets().open(&format!("ws://{address}/")).await.unwrap();
    socket.send(Text::plain("hello")).await.unwrap();
    assert_eq!(socket.receive().await, Frame::Text("echo hello".to_owned()));
    assert_eq!(socket.receive().await, Frame::Closed(Some(4004)));
    assert_eq!(socket.receive().await, Frame::Closed(None));
    assert!(socket.send(Text::plain("late")).await.is_err());
    server.await.unwrap();
    let raw = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let raw_address = raw.local_addr().unwrap();
    let refusing = tokio::spawn(async move {
        let (mut stream, _) = raw.accept().await.unwrap();
        let mut buffer = [0_u8; 512];
        let _ = stream.read(&mut buffer).await;
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n")
            .await
            .unwrap();
    });
    let unopened = sockets().open(&format!("ws://{raw_address}/")).await;
    assert!(matches!(unopened, Err(HttpError::Unreachable { .. })));
    refusing.await.unwrap();
}

#[tokio::test]
async fn a_silent_connection_is_pinged_then_lost_and_one_that_never_opens_is_not_opened() {
    let silence = Duration::from_millis(100);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        socket.send(Message::Pong(Vec::new().into())).await.unwrap();
        let pinged = socket.next().await.unwrap().unwrap();
        assert!(matches!(pinged, Message::Ping(_)), "{pinged:?}");
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let mut socket = sockets()
        .silent_for(silence)
        .open(&format!("ws://{address}/"))
        .await
        .unwrap();
    let started = Instant::now();
    assert_eq!(socket.receive().await, Frame::Closed(None));
    assert!(started.elapsed() >= silence * 2, "{:?}", started.elapsed());
    server.abort();
    let mute = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mute_address = mute.local_addr().unwrap();
    let unopened = sockets()
        .silent_for(silence)
        .open(&format!("ws://{mute_address}/"))
        .await;
    assert!(
        matches!(&unopened, Err(HttpError::Unreachable { reason }) if reason.contains("did not open")),
        "{unopened:?}"
    );
    drop(mute);
}

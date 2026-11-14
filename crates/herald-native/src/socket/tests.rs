use super::*;

fn silent_for(silence: Duration) -> NativeSockets {
    NativeSockets::under(["discord.gg", "slack.com"], silence).unwrap()
}

fn sockets() -> NativeSockets {
    silent_for(Duration::from_secs(30))
}

/// How long a test waits for a frame before it takes the socket to hang.
const LOST_BY: Duration = Duration::from_secs(3);

async fn heard(socket: &mut NativeSocket) -> Frame {
    timeout(LOST_BY, socket.receive())
        .await
        .expect("a frame or a close within the bound")
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
    socket.send(SecretText::plain("hello")).await.unwrap();
    assert_eq!(
        heard(&mut socket).await,
        Frame::Text("echo hello".to_owned())
    );
    assert_eq!(heard(&mut socket).await, Frame::Closed(Some(4004)));
    assert_eq!(heard(&mut socket).await, Frame::Closed(None));
    assert!(socket.send(SecretText::plain("late")).await.is_err());
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
    let mut socket = silent_for(silence)
        .open(&format!("ws://{address}/"))
        .await
        .unwrap();
    let started = Instant::now();
    assert_eq!(heard(&mut socket).await, Frame::Closed(None));
    let lost = started.elapsed();
    assert!(lost >= silence * 2 && lost < LOST_BY, "{lost:?}");
    server.abort();
    let mute = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mute_address = mute.local_addr().unwrap();
    let unopened = timeout(
        LOST_BY,
        silent_for(silence).open(&format!("ws://{mute_address}/")),
    )
    .await
    .expect("an answer within the bound");
    assert!(
        matches!(&unopened, Err(HttpError::Unreachable { reason }) if reason.contains("did not open")),
        "{unopened:?}"
    );
    drop(mute);
}

//! A loopback server answering each connection with the next scripted
//! response, and keeping each request it read. A client that hangs up
//! before the answer is written, as one refusing a long body does, ends
//! that connection alone.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

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

pub(crate) async fn serve(answers: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for answer in answers {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(request_of(&mut stream).await);
            if stream.write_all(&answer).await.is_ok() {
                let _ = stream.shutdown().await;
            }
        }
        requests
    });
    (base, server)
}

pub(crate) fn answer(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n",
        body.len()
    );
    [head.as_bytes(), body].concat()
}

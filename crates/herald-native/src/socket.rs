use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use herald_compose::SocketReach;
use herald_ports::{Frame, HttpError, SecretText, Socket, Sockets};
use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout, timeout_at};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use crate::http::{BuildError, tls};

/// How long a connection hears nothing before it is pinged where nothing
/// else is said.
const SILENCE: Duration = Duration::from_secs(30);

/// `Sockets` over `tokio-tungstenite` with rustls and the Mozilla roots,
/// opening `wss` connections under the domains it is given alone, and plain
/// `ws` only on a loopback host (ADR-0007). A connection that hears nothing
/// for its silence is pinged, and taken as lost where it hears nothing for
/// as long again; one that does not open within its silence is not opened.
#[derive(Debug, Clone)]
pub struct NativeSockets {
    reach: SocketReach,
    tls: Arc<ClientConfig>,
    silence: Duration,
}

/// One connection [`NativeSockets`] opened.
#[derive(Debug)]
pub struct NativeSocket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    silence: Duration,
    heard: Instant,
    pinged: bool,
}

impl NativeSockets {
    /// Sockets reaching only hosts under `domains`.
    ///
    /// # Errors
    ///
    /// When TLS cannot be set up.
    pub fn under<'a>(domains: impl IntoIterator<Item = &'a str>) -> Result<Self, BuildError> {
        Ok(Self {
            reach: SocketReach::under(domains),
            tls: Arc::new(tls(rustls::crypto::ring::default_provider())?),
            silence: SILENCE,
        })
    }

    /// These sockets, each connection pinged after `silence` and lost after
    /// as long again.
    #[must_use]
    pub fn silent_for(self, silence: Duration) -> Self {
        Self { silence, ..self }
    }
}

impl Sockets for NativeSockets {
    type Socket = NativeSocket;

    async fn open(&self, url: &str) -> Result<NativeSocket, HttpError> {
        let parsed = self.reach.admitted(url)?;
        let connecting = tokio_tungstenite::connect_async_tls_with_config(
            parsed.as_str(),
            None,
            false,
            Some(Connector::Rustls(Arc::clone(&self.tls))),
        );
        let (stream, _) = timeout(self.silence, connecting)
            .await
            .map_err(|_| {
                HttpError::unreachable(format_args!("it did not open within {:?}", self.silence))
            })?
            .map_err(HttpError::unreachable)?;
        Ok(NativeSocket {
            stream,
            silence: self.silence,
            heard: Instant::now(),
            pinged: false,
        })
    }
}

impl Socket for NativeSocket {
    async fn send(&mut self, frame: SecretText) -> Result<(), HttpError> {
        self.stream
            .send(Message::text(frame.expose()))
            .await
            .map_err(HttpError::unreachable)
    }

    async fn receive(&mut self) -> Frame {
        loop {
            let message = match timeout_at(self.heard + self.silence, self.stream.next()).await {
                Err(_) if self.pinged => return Frame::Closed(None),
                Err(_) => {
                    self.pinged = true;
                    self.heard = Instant::now();
                    let _ = self.stream.send(Message::Ping(Vec::new().into())).await;
                    continue;
                }
                Ok(Some(Ok(message))) => message,
                Ok(Some(Err(_)) | None) => return Frame::Closed(None),
            };
            self.heard = Instant::now();
            self.pinged = false;
            match message {
                Message::Text(text) => return Frame::Text(text.as_str().to_owned()),
                Message::Close(close) => {
                    return Frame::Closed(close.map(|close| u16::from(close.code)));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;

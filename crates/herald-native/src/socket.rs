use std::sync::Arc;

use futures_util::{SinkExt as _, StreamExt as _};
use herald_ports::{Frame, HttpError, Socket, Sockets, Text};
use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};
use url::{Host, Url};

use crate::http::{BuildError, tls};

/// `Sockets` over `tokio-tungstenite` with rustls and the Mozilla roots,
/// opening `wss` connections under the domains it is given alone, and plain
/// `ws` only on a loopback host (ADR-0007).
#[derive(Debug, Clone)]
pub struct NativeSockets {
    domains: Vec<String>,
    tls: Arc<ClientConfig>,
}

/// One connection [`NativeSockets`] opened.
#[derive(Debug)]
pub struct NativeSocket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl NativeSockets {
    /// Sockets reaching only hosts under `domains`.
    ///
    /// # Errors
    ///
    /// When TLS cannot be set up.
    pub fn under<'a>(domains: impl IntoIterator<Item = &'a str>) -> Result<Self, BuildError> {
        Ok(Self {
            domains: domains.into_iter().map(ToOwned::to_owned).collect(),
            tls: Arc::new(tls(rustls::crypto::ring::default_provider())?),
        })
    }

    fn allows(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        let loopback = match url.host() {
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            Some(Host::Domain(name)) => name == "localhost",
            None => false,
        };
        match url.scheme() {
            "wss" => self.domains.iter().any(|domain| {
                host == domain
                    || host
                        .strip_suffix(domain.as_str())
                        .is_some_and(|rest| rest.ends_with('.'))
            }),
            "ws" => loopback,
            _ => false,
        }
    }
}

impl Sockets for NativeSockets {
    type Socket = NativeSocket;

    async fn open(&self, url: &str) -> Result<NativeSocket, HttpError> {
        let refused = || HttpError::Refused {
            url: url.split('?').next().unwrap_or_default().to_owned(),
        };
        let parsed = Url::parse(url).map_err(|_| refused())?;
        if !self.allows(&parsed) {
            return Err(refused());
        }
        let (stream, _) = tokio_tungstenite::connect_async_tls_with_config(
            parsed.as_str(),
            None,
            false,
            Some(Connector::Rustls(Arc::clone(&self.tls))),
        )
        .await
        .map_err(|error| HttpError::Unreachable {
            reason: error.to_string(),
        })?;
        Ok(NativeSocket { stream })
    }
}

impl Socket for NativeSocket {
    async fn send(&mut self, frame: Text) -> Result<(), HttpError> {
        self.stream
            .send(Message::text(frame.expose()))
            .await
            .map_err(|error| HttpError::Unreachable {
                reason: error.to_string(),
            })
    }

    async fn receive(&mut self) -> Frame {
        loop {
            match self.stream.next().await {
                Some(Ok(Message::Text(text))) => return Frame::Text(text.as_str().to_owned()),
                Some(Ok(Message::Close(close))) => {
                    return Frame::Closed(close.map(|close| u16::from(close.code)));
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return Frame::Closed(None),
            }
        }
    }
}

#[cfg(test)]
mod tests;

use std::future::Future;

use crate::http::HttpError;
use crate::secret::Text;

/// What a socket received next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A text frame.
    Text(String),
    /// The other side closed the connection with this code, or it was lost
    /// with none.
    Closed(Option<u16>),
}

/// One WebSocket connection, carrying text frames.
pub trait Socket: Send {
    /// Sends `frame`.
    fn send(&mut self, frame: Text) -> impl Future<Output = Result<(), HttpError>> + Send;

    /// The next frame, or how the connection closed.
    fn receive(&mut self) -> impl Future<Output = Frame> + Send;
}

/// What opens WebSocket connections, to the hosts it may reach alone
/// (ADR-0007).
pub trait Sockets: Sync {
    /// The connections it opens.
    type Socket: Socket;

    /// A connection to `url`.
    fn open(&self, url: &str) -> impl Future<Output = Result<Self::Socket, HttpError>> + Send;
}

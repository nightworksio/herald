use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use herald_ports::{Frame, HttpError, Socket, Sockets, Text};

use crate::lock;

/// One scripted connection: the frames it receives in turn, or the error
/// opening it fails with.
pub type Connection = Result<Vec<Frame>, HttpError>;

/// Sockets that open one scripted connection after another, each after
/// giving the rest of its task a turn: each the frames it receives in turn,
/// after which it waits for ever, or the error opening it fails with. Once
/// the script is spent, nothing can be reached.
#[derive(Debug, Clone, Default)]
pub struct ScriptedSockets {
    connections: Arc<Mutex<VecDeque<Connection>>>,
    opened: Arc<Mutex<Vec<String>>>,
    sent: Arc<Mutex<Vec<String>>>,
    taking: Option<usize>,
}

/// One connection [`ScriptedSockets`] opened.
#[derive(Debug)]
pub struct ScriptedSocket {
    frames: VecDeque<Frame>,
    sent: Arc<Mutex<Vec<String>>>,
    taking: Option<usize>,
}

impl ScriptedSockets {
    /// Sockets opening `connections` in turn.
    #[must_use]
    pub fn opening(connections: Vec<Connection>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(connections.into())),
            ..Self::default()
        }
    }

    /// The same sockets, each taking the first `taken` frames sent on it
    /// and refusing every later one.
    #[must_use]
    pub fn refusing_after(self, taken: usize) -> Self {
        Self {
            taking: Some(taken),
            ..self
        }
    }

    /// Every address a connection was opened to, in turn.
    #[must_use]
    pub fn opened(&self) -> Vec<String> {
        lock(&self.opened).clone()
    }

    /// Every frame sent, in turn, with its secrets shown.
    #[must_use]
    pub fn sent(&self) -> Vec<String> {
        lock(&self.sent).clone()
    }
}

/// A future pending once, its task woken at once, then ready: the turn a
/// real connection gives the rest of its task before it opens.
#[derive(Default)]
struct Turn(bool);

impl Future for Turn {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        context.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Sockets for ScriptedSockets {
    type Socket = ScriptedSocket;

    async fn open(&self, url: &str) -> Result<ScriptedSocket, HttpError> {
        Turn::default().await;
        lock(&self.opened).push(url.to_owned());
        let next = lock(&self.connections).pop_front();
        let frames = next.unwrap_or_else(|| {
            Err(HttpError::Unreachable {
                reason: "no connection is scripted".to_owned(),
            })
        })?;
        Ok(ScriptedSocket {
            frames: frames.into(),
            sent: Arc::clone(&self.sent),
            taking: self.taking,
        })
    }
}

impl Socket for ScriptedSocket {
    async fn send(&mut self, frame: Text) -> Result<(), HttpError> {
        match &mut self.taking {
            Some(0) => {
                return Err(HttpError::Unreachable {
                    reason: "the connection refuses what is sent".to_owned(),
                });
            }
            Some(left) => *left -= 1,
            None => {}
        }
        lock(&self.sent).push(frame.expose());
        Ok(())
    }

    async fn receive(&mut self) -> Frame {
        match self.frames.pop_front() {
            Some(frame) => frame,
            None => std::future::pending().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_connection_receives_its_frames_and_the_spent_script_reaches_nothing() {
        let sockets = ScriptedSockets::opening(vec![
            Ok(vec![Frame::Text("a".to_owned()), Frame::Closed(Some(1000))]),
            Err(HttpError::TimedOut),
        ]);
        let mut socket = pollster::block_on(sockets.open("wss://one")).unwrap();
        assert_eq!(
            pollster::block_on(socket.receive()),
            Frame::Text("a".to_owned())
        );
        assert_eq!(
            pollster::block_on(socket.receive()),
            Frame::Closed(Some(1000))
        );
        assert_eq!(pollster::block_on(socket.send(Text::plain("b"))), Ok(()));
        assert_eq!(sockets.sent(), ["b"]);
        assert_eq!(
            pollster::block_on(sockets.open("wss://two")).err(),
            Some(HttpError::TimedOut)
        );
        assert!(matches!(
            pollster::block_on(sockets.open("wss://three")),
            Err(HttpError::Unreachable { .. })
        ));
        assert_eq!(sockets.opened(), ["wss://one", "wss://two", "wss://three"]);
    }

    #[test]
    fn refusing_sockets_refuse_every_frame_after_those_they_take() {
        let sockets = ScriptedSockets::opening(vec![Ok(Vec::new())]).refusing_after(1);
        let mut socket = pollster::block_on(sockets.open("wss://one")).unwrap();
        assert!(pollster::block_on(socket.send(Text::plain("a"))).is_ok());
        assert!(pollster::block_on(socket.send(Text::plain("b"))).is_err());
        assert_eq!(sockets.sent(), ["a"]);
    }

    #[test]
    fn a_connection_whose_frames_are_spent_waits_for_ever() {
        let sockets = ScriptedSockets::opening(vec![Ok(Vec::new())]);
        let mut socket = pollster::block_on(sockets.open("wss://one")).unwrap();
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        let mut receiving = std::pin::pin!(socket.receive());
        assert!(receiving.as_mut().poll(&mut context).is_pending());
    }

    #[test]
    fn a_connection_opens_after_its_task_has_had_a_turn() {
        let sockets = ScriptedSockets::opening(vec![Ok(Vec::new())]);
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        let mut opening = std::pin::pin!(sockets.open("wss://one"));
        assert!(opening.as_mut().poll(&mut context).is_pending());
        assert_eq!(sockets.opened(), Vec::<String>::new());
        assert!(opening.as_mut().poll(&mut context).is_ready());
        assert_eq!(sockets.opened(), ["wss://one"]);
    }
}

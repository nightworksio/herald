use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use herald_compose::{Composed, SocketReach, told};
use herald_core::DestinationName;
use herald_flows::{Job, Taker};
use herald_ports::{Frame, HttpError, Socket, Sockets, Text, Timer};
use js_sys::Promise;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::bindings::{SocketObject, SocketsObject, log, set_timeout};
use super::host::JsHost;
use super::{member, settled};
use crate::numbers::count;

/// The WebSocket connections the host opens, to what sessions may reach.
pub(crate) struct JsSockets {
    pub(crate) opener: SendWrapper<SocketsObject>,
    pub(crate) reach: SocketReach,
}

/// One connection the host opened.
pub(crate) struct JsSocket(SendWrapper<SocketObject>);

impl Sockets for JsSockets {
    type Socket = JsSocket;

    fn open(&self, url: &str) -> impl Future<Output = Result<JsSocket, HttpError>> + Send {
        let admitted = self.reach.admitted(url);
        let opener = self.opener.clone();
        SendWrapper::new(async move {
            let url = admitted?;
            let socket = settled(opener.open(url.as_str()))
                .await
                .map_err(|reason| HttpError::Unreachable { reason })?;
            Ok(JsSocket(SendWrapper::new(socket.unchecked_into())))
        })
    }
}

impl Socket for JsSocket {
    fn send(&mut self, frame: Text) -> impl Future<Output = Result<(), HttpError>> + Send {
        let socket = self.0.clone();
        SendWrapper::new(async move {
            settled(socket.send(&frame.expose()))
                .await
                .map(|_| ())
                .map_err(|reason| HttpError::Unreachable { reason })
        })
    }

    fn receive(&mut self) -> impl Future<Output = Frame> + Send {
        let socket = self.0.clone();
        SendWrapper::new(async move {
            let Ok(received) = settled(socket.receive()).await else {
                return Frame::Closed(None);
            };
            member(&received, "text").as_string().map_or_else(
                || {
                    Frame::Closed(
                        member(&received, "closed")
                            .as_f64()
                            .and_then(count)
                            .and_then(|code| u16::try_from(code).ok()),
                    )
                },
                Frame::Text,
            )
        })
    }
}

/// Waiting, with the host's `setTimeout`.
pub(crate) struct JsTimer;

impl Timer for JsTimer {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        let milliseconds =
            u32::try_from(duration.as_millis()).map_or(f64::from(u32::MAX), f64::from);
        let waited = Promise::new(&mut |resolve, _| {
            let _ = set_timeout(&resolve, milliseconds);
        });
        SendWrapper::new(async move {
            let _ = JsFuture::from(waited).await;
        })
    }
}

/// Where the sessions hand each command's work: done on the host's event
/// loop, at most so many at once.
pub(crate) struct JsTaker {
    pub(crate) composed: Rc<Composed<JsHost>>,
    pub(crate) working: Rc<Cell<usize>>,
    pub(crate) most: usize,
}

impl Taker for JsTaker {
    type Place = ();

    fn place(&self) -> Option<()> {
        let working = self.working.get();
        (working < self.most).then(|| self.working.set(working + 1))
    }

    fn fill(&self, (): (), job: Job) {
        let composed = Rc::clone(&self.composed);
        let working = Rc::clone(&self.working);
        spawn_local(async move {
            for line in told::worked(&composed.work(job).await) {
                log(&line);
            }
            working.set(working.get().saturating_sub(1));
        });
    }

    fn said(&self, destination: &DestinationName, said: String) {
        log(&format!("{}: {said}", destination.as_str()));
    }
}

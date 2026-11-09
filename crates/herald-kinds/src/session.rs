use herald_core::{Asked, DestinationName, Place};
use herald_ports::{Request, Response, Text};
use jiff::SignedDuration;

use crate::Replying;

/// How a session connects, each time it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connect {
    /// A socket opened at the URL.
    Socket(String),
    /// A socket opened at the URL the answer to the request names, sent
    /// with the session's own opening credential.
    Opening(Request),
    /// The request, sent and its answer read: one poll.
    Poll(Request),
}

/// What a session does as it hears from its platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Send the frame; where it cannot be sent, connect again once every
    /// effect heard with it is done.
    Send(Text),
    /// Send the frame acknowledging what was heard; where it cannot be
    /// sent, connect again at once, and do nothing more of what was heard.
    Acknowledge(Text),
    /// Beat every interval from now on.
    Beat(SignedDuration),
    /// The session is taken up: its next wait starts from the first.
    Ready,
    /// A command, and where its answer goes.
    Asked(Asked, Replying),
    /// A member typing a project at `place`, to be offered the projects
    /// announced there that start with `typed`.
    Completing {
        /// Where it is typed.
        place: Place,
        /// What is typed so far.
        typed: String,
        /// Where the offer goes.
        replying: Replying,
    },
    /// Connect again.
    Reconnect,
    /// Stop, and why.
    Stop(String),
}

/// A session a destination holds where it carries commands without posting
/// them to herald: what it does as it hears, kept apart from the sockets,
/// requests and waits that carry it (ADR-0016, ADR-0017).
pub trait Session: Send {
    /// How it connects now.
    fn connect(&mut self) -> Connect;

    /// The URL of the socket the answer to its opening request names.
    ///
    /// # Errors
    ///
    /// Why no socket can be opened from it.
    fn opened(&mut self, _response: &Response) -> Result<String, String> {
        Err("it opens no socket by a request".to_owned())
    }

    /// What it does on hearing the text frame `text` for `destination`.
    fn heard(&mut self, _destination: &DestinationName, _text: &str) -> Vec<Effect> {
        vec![]
    }

    /// What it does when its socket is closed with `code`, or lost.
    fn closed(&mut self, _code: Option<u16>) -> Vec<Effect> {
        vec![Effect::Reconnect]
    }

    /// What it does when its beat is due.
    fn beat(&mut self) -> Vec<Effect> {
        vec![]
    }

    /// What it does with the answer to a poll for `destination`.
    ///
    /// # Errors
    ///
    /// Why the answer could not be read.
    fn polled(
        &mut self,
        _destination: &DestinationName,
        _response: &Response,
    ) -> Result<Vec<Effect>, String> {
        Ok(vec![])
    }

    /// What a failure to connect is said as, before its reason.
    fn unreachable(&self) -> &'static str;
}

#[cfg(test)]
mod tests;

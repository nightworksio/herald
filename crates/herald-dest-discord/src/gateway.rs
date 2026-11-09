//! Discord's gateway as herald holds it: a session that identifies as the
//! bot, beats when Discord asks, resumes where it can, and hands over each
//! interaction it carries (ADR-0016). Nothing here does I/O; a driver sends
//! what it is told to and passes on what Discord sends.

use herald_core::{DestinationName, socket_under};
use herald_ports::{Secret, Text};
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::interact::{Interacting, Received};

/// Where a session starts where Discord named nowhere to resume.
pub const GATEWAY: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
/// What a resumed session's address is asked for.
const QUERY: &str = "/?v=10&encoding=json";
/// An event Discord dispatches.
const DISPATCH: u64 = 0;
/// A beat, sent by either side.
const HEARTBEAT: u64 = 1;
/// The bot naming itself.
const IDENTIFY: u64 = 2;
/// A session taken up again.
const RESUME: u64 = 6;
/// Discord asking the session to reconnect.
const RECONNECT: u64 = 7;
/// Discord refusing the session.
const INVALID_SESSION: u64 = 9;
/// Discord greeting the session with how often to beat.
const HELLO: u64 = 10;
/// Discord acknowledging a beat.
const HEARTBEAT_ACK: u64 = 11;
/// The close codes after which reconnecting cannot help: the token was
/// refused, the shard or the API version is wrong, or the intents are not
/// allowed.
const FATAL: [u16; 6] = [4004, 4010, 4011, 4012, 4013, 4014];
/// The close codes after which the session cannot be taken up again: the
/// sequence sent was wrong, or the session timed out.
const UNRESUMABLE: [u16; 2] = [4007, 4009];

/// What a driver does next, as the session tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Send this frame.
    Send(Text),
    /// Beat this often from now on.
    Beat(SignedDuration),
    /// Answer this interaction.
    Received(Box<Received>),
    /// The session is identified, or taken up again: what a driver waits
    /// before it reconnects starts over.
    Ready,
    /// Close the connection and open another, taking the session up again
    /// where `resume`.
    Reconnect {
        /// Whether the session is taken up again.
        resume: bool,
    },
    /// Stop: reconnecting cannot help, for this reason.
    Stop(String),
}

/// One session with Discord's gateway.
#[derive(Debug, Clone)]
pub struct Gateway {
    token: Secret,
    domains: Vec<String>,
    os: String,
    sequence: Option<u64>,
    session: Option<String>,
    resume: Option<String>,
    acknowledged: bool,
    resuming: bool,
}

impl Gateway {
    /// A session for the bot whose token is `token`, resuming only under
    /// `domains`.
    #[must_use]
    pub fn new(token: Secret, domains: &[&str]) -> Self {
        Self {
            token,
            domains: domains.iter().map(|domain| (*domain).to_owned()).collect(),
            os: std::env::consts::OS.to_owned(),
            sequence: None,
            session: None,
            resume: None,
            acknowledged: true,
            resuming: false,
        }
    }

    /// The session, identifying to Discord as running on `os` where it
    /// names one, in place of the host's own.
    #[must_use]
    pub fn identifying_as(self, os: Option<&str>) -> Self {
        match os {
            Some(os) => Self {
                os: os.to_owned(),
                ..self
            },
            None => self,
        }
    }

    /// Where the next connection opens: where Discord said to resume, while
    /// the session is taken up again, or the gateway.
    #[must_use]
    pub fn url(&self) -> String {
        match (&self.resume, self.resuming) {
            (Some(resume), true) => format!("{}{QUERY}", resume.trim_end_matches('/')),
            _ => GATEWAY.to_owned(),
        }
    }

    /// What a driver does with the `frame` Discord sent, an interaction read
    /// by `interacting` for `destination`.
    pub fn received(
        &mut self,
        interacting: &Interacting,
        destination: &DestinationName,
        frame: &str,
    ) -> Vec<Effect> {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return Vec::new();
        };
        if let Some(sequence) = value.get("s").and_then(Value::as_u64) {
            self.sequence = Some(sequence);
        }
        match value.get("op").and_then(Value::as_u64) {
            Some(HELLO) => {
                let every = value
                    .pointer("/d/heartbeat_interval")
                    .and_then(Value::as_i64)
                    .map_or(SignedDuration::from_secs(41), SignedDuration::from_millis);
                self.acknowledged = true;
                vec![Effect::Beat(every), Effect::Send(self.introduced())]
            }
            Some(HEARTBEAT) => vec![Effect::Send(self.heartbeat())],
            Some(HEARTBEAT_ACK) => {
                self.acknowledged = true;
                Vec::new()
            }
            Some(RECONNECT) => vec![self.reconnect(true)],
            Some(INVALID_SESSION) => {
                let resumable = value.get("d").and_then(Value::as_bool) == Some(true);
                vec![self.reconnect(resumable)]
            }
            Some(DISPATCH) => self.dispatched(interacting, destination, &value),
            _ => Vec::new(),
        }
    }

    fn dispatched(
        &mut self,
        interacting: &Interacting,
        destination: &DestinationName,
        value: &Value,
    ) -> Vec<Effect> {
        let data = value.get("d").unwrap_or(&Value::Null);
        match value.get("t").and_then(Value::as_str) {
            Some("READY") => {
                self.session = data
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                let domains: Vec<&str> = self.domains.iter().map(String::as_str).collect();
                self.resume = data
                    .get("resume_gateway_url")
                    .and_then(Value::as_str)
                    .filter(|url| socket_under(url, &domains))
                    .map(ToOwned::to_owned);
                vec![Effect::Ready]
            }
            Some("RESUMED") => vec![Effect::Ready],
            Some("INTERACTION_CREATE") => match interacting.read(destination, data) {
                Ok(Received::Ignored) | Err(_) => Vec::new(),
                Ok(received) => vec![Effect::Received(Box::new(received))],
            },
            _ => Vec::new(),
        }
    }

    fn introduced(&mut self) -> Text {
        let resuming = std::mem::take(&mut self.resuming);
        if let (Some(session), Some(sequence), true) = (&self.session, self.sequence, resuming) {
            return framed(
                &json!({ "op": RESUME, "d": { "token": TOKEN, "session_id": session, "seq": sequence } }),
                &self.token,
            );
        }
        self.session = None;
        self.sequence = None;
        framed(
            &json!({ "op": IDENTIFY, "d": {
                "token": TOKEN,
                "intents": 0,
                "properties": { "os": self.os, "browser": "herald", "device": "herald" },
            }}),
            &self.token,
        )
    }

    fn heartbeat(&self) -> Text {
        Text::plain(json!({ "op": HEARTBEAT, "d": self.sequence }).to_string())
    }

    fn reconnect(&mut self, resume: bool) -> Effect {
        self.resuming = resume && self.session.is_some();
        Effect::Reconnect {
            resume: self.resuming,
        }
    }

    /// What a driver does when a beat falls due: beats, or reconnects where
    /// Discord did not acknowledge the last one.
    pub fn beat(&mut self) -> Effect {
        if !self.acknowledged {
            return self.reconnect(true);
        }
        self.acknowledged = false;
        Effect::Send(self.heartbeat())
    }

    /// What a driver does once Discord closed the connection with `code`,
    /// or it was lost with none: stops where reconnecting cannot help,
    /// starts a session afresh where this one cannot be taken up again, and
    /// otherwise takes it up again.
    pub fn closed(&mut self, code: Option<u16>) -> Effect {
        match code {
            Some(code) if FATAL.contains(&code) => Effect::Stop(format!(
                "Discord's gateway closed the connection with {code}, which reconnecting cannot help: check the bot token and the application's intents"
            )),
            Some(code) if UNRESUMABLE.contains(&code) => self.reconnect(false),
            _ => self.reconnect(true),
        }
    }
}

/// Where a frame's token goes, replaced by the token itself as a secret so
/// that no frame shown in a log carries it.
const TOKEN: &str = "\u{0}herald-token\u{0}";

fn framed(value: &Value, token: &Secret) -> Text {
    let written = value.to_string();
    let placeholder = serde_json::to_string(TOKEN).unwrap_or_default();
    match written.split_once(&placeholder) {
        Some((before, after)) => Text::plain(format!("{before}\""))
            .then_secret(token.clone())
            .then(format!("\"{after}")),
        None => Text::plain(written),
    }
}

#[cfg(test)]
mod tests;

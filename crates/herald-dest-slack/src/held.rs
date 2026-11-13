use herald_core::DestinationName;
use herald_kinds::{Connect, Effect, Replying, Session};
use herald_ports::{Response, Secret, SecretText};

use crate::interact::{Interacting, Received};
use crate::interactions::Via;
use crate::socket::{Ended, opened};

/// What is said where Slack turned Socket Mode off for the app.
const DISABLED: &str = "Slack turned Socket Mode off for the app, so its commands cannot be carried; turn Socket Mode on in the app's settings";

/// Slack's Socket Mode, opened with the destination's app-level token.
pub(crate) struct Socketed<'a> {
    interacting: &'a Interacting,
    app: &'a Secret,
}

impl<'a> Socketed<'a> {
    /// The Socket Mode session `interacting` holds, where it carries
    /// commands over one and has an app-level token to open it with.
    pub(crate) fn of(interacting: &'a Interacting) -> Option<Self> {
        if interacting.via() != Via::Socket {
            return None;
        }
        Some(Self {
            interacting,
            app: interacting.app_token()?,
        })
    }
}

impl Session for Socketed<'_> {
    fn connect(&mut self) -> Connect {
        Connect::Opening(self.interacting.opening(self.app))
    }

    fn opened(&mut self, response: &Response) -> Result<String, String> {
        opened(response, &self.interacting.session_domains())
    }

    fn heard(&mut self, destination: &DestinationName, text: &str) -> Vec<Effect> {
        let enveloped = self.interacting.envelope(destination, text);
        let mut effects = Vec::new();
        if enveloped.greeted {
            effects.push(Effect::Ready);
        }
        if let Some(acknowledgement) = enveloped.acknowledgement {
            effects.push(Effect::Acknowledge(SecretText::plain(acknowledgement)));
        }
        if let Some(Received::Asked { asked, replying }) = enveloped.received {
            effects.push(Effect::Asked(*asked, Replying::new(replying)));
        }
        match enveloped.ended {
            Some(Ended::Reconnect) => effects.push(Effect::Reconnect),
            Some(Ended::Disabled) => effects.push(Effect::Stop(DISABLED.to_owned())),
            None => {}
        }
        effects
    }

    fn unreachable(&self) -> &'static str {
        "Slack's Socket Mode could not be opened"
    }
}

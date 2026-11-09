use herald_core::DestinationName;
use herald_kinds::{Connect, Effect, Replying, Session};

use crate::gateway::{self, Gateway};
use crate::interact::{Interacting, Received};
use crate::interactions::Via;

/// Discord's gateway, joined as the destination's bot.
pub(crate) struct Joined<'a> {
    interacting: &'a Interacting,
    gateway: Gateway,
}

impl<'a> Joined<'a> {
    /// The gateway `interacting` joins, where it carries commands over it
    /// and has a bot to join as.
    pub(crate) fn of(interacting: &'a Interacting) -> Option<Self> {
        if interacting.via() != Via::Gateway {
            return None;
        }
        let gateway = Gateway::new(interacting.bot()?.clone(), &interacting.session_domains())
            .identifying_as(interacting.os());
        Some(Self {
            interacting,
            gateway,
        })
    }
}

fn effect(effect: gateway::Effect) -> Option<Effect> {
    Some(match effect {
        gateway::Effect::Send(frame) => Effect::Send(frame),
        gateway::Effect::Beat(interval) => Effect::Beat(interval),
        gateway::Effect::Ready => Effect::Ready,
        gateway::Effect::Received(received) => match *received {
            Received::Asked { asked, replying } => Effect::Asked(asked, Replying::new(replying)),
            Received::Completing {
                place,
                typed,
                replying,
            } => Effect::Completing {
                place,
                typed,
                replying: Replying::new(replying),
            },
            Received::Ping | Received::Ignored => return None,
        },
        gateway::Effect::Reconnect { .. } => Effect::Reconnect,
        gateway::Effect::Stop(reason) => Effect::Stop(reason),
    })
}

impl Session for Joined<'_> {
    fn connect(&mut self) -> Connect {
        Connect::Socket(self.gateway.url())
    }

    fn heard(&mut self, destination: &DestinationName, text: &str) -> Vec<Effect> {
        self.gateway
            .received(self.interacting, destination, text)
            .into_iter()
            .filter_map(effect)
            .collect()
    }

    fn closed(&mut self, code: Option<u16>) -> Vec<Effect> {
        effect(self.gateway.closed(code)).into_iter().collect()
    }

    fn beat(&mut self) -> Vec<Effect> {
        effect(self.gateway.beat()).into_iter().collect()
    }

    fn unreachable(&self) -> &'static str {
        "Discord's gateway could not be reached"
    }
}

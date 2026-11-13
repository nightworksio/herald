//! herald's answers to an interaction: the responses an endpoint gives, and
//! the requests that answer through the interaction's webhook.

use herald_ports::{Method, Request, SecretText};
use serde_json::{Value, json};

use super::{Interacting, Replying};
use crate::request::json;

/// The answer to a ping.
const PONG: u64 = 1;
/// An answer that says herald is working, to be edited once it has worked.
const DEFERRED: u64 = 5;
/// An answer listing the completions of what a member typed.
const CHOICES: u64 = 8;
/// The flag of a message the asker alone sees.
const EPHEMERAL: u64 = 64;
/// The longest message Discord takes, in characters.
pub const MESSAGE: usize = 2000;

impl Interacting {
    fn callback(&self, replying: &Replying, body: String) -> Request {
        let url = SecretText::plain(format!("{}/interactions/{}/", self.api, replying.id))
            .then_secret(replying.token.clone())
            .then("/callback");
        json(Method::Post, url, None, body)
    }

    fn hook(&self, replying: &Replying, rest: &str) -> SecretText {
        SecretText::plain(format!(
            "{}/webhooks/{}/",
            self.api, self.interactions.application
        ))
        .then_secret(replying.token.clone())
        .then(rest)
    }

    /// The request that answers the interaction `replying` names as one that
    /// is being worked, seen by the asker alone unless `public`: the answer
    /// an endpoint gives in its response, sent where the gateway carried the
    /// interaction.
    #[must_use]
    pub(crate) fn defer(&self, replying: &Replying, public: bool) -> Request {
        self.callback(replying, deferred(public))
    }

    /// The request that answers the interaction `replying` names with
    /// `projects` as the completions of what was typed, sent where the
    /// gateway carried the interaction.
    #[must_use]
    pub fn complete(&self, replying: &Replying, projects: &[String]) -> Request {
        self.callback(replying, choices(projects))
    }

    /// The requests that answer the interaction `replying` names with
    /// `parts`, each within [`MESSAGE`]: the first in place of the answer
    /// that said herald was working, each other as a follow-up, seen by the
    /// asker alone unless `public`. No answer mentions anyone.
    #[must_use]
    pub fn answered(&self, replying: &Replying, parts: &[String], public: bool) -> Vec<Request> {
        let flags = if public { 0 } else { EPHEMERAL };
        parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let (method, rest, body) = if index == 0 {
                    (
                        Method::Patch,
                        "/messages/@original",
                        json!({ "content": part, "allowed_mentions": { "parse": [] } }),
                    )
                } else {
                    (
                        Method::Post,
                        "",
                        json!({
                            "content": part,
                            "flags": flags,
                            "allowed_mentions": { "parse": [] },
                        }),
                    )
                };
                json(method, self.hook(replying, rest), None, body.to_string())
            })
            .collect()
    }
}

/// The response that answers a ping.
#[must_use]
pub(crate) fn pong() -> String {
    json!({ "type": PONG }).to_string()
}

/// The response that says herald is working on an interaction, seen by the
/// asker alone unless `public`.
#[must_use]
pub fn deferred(public: bool) -> String {
    if public {
        json!({ "type": DEFERRED }).to_string()
    } else {
        json!({ "type": DEFERRED, "data": { "flags": EPHEMERAL } }).to_string()
    }
}

/// The response that lists `projects` as the completions of what a member
/// typed.
#[must_use]
pub fn choices(projects: &[String]) -> String {
    let choices: Vec<Value> = projects
        .iter()
        .map(|project| json!({ "name": project, "value": project }))
        .collect();
    json!({ "type": CHOICES, "data": { "choices": choices } }).to_string()
}

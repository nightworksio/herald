//! The roles a subscription gives: given, taken, listed and made.

use herald_core::Phrase;
use herald_ports::{Method, Request, Response, Text};
use serde_json::{Value, json};

use super::{Interacting, text, took};
use crate::interactions::snowflake;

impl Interacting {
    /// The request that gives `user` the role `role` in `guild` where `give`,
    /// or takes it away; none without a bot token or where an id is not
    /// written as Discord writes one.
    #[must_use]
    pub fn role(&self, guild: &str, user: &str, role: &str, give: bool) -> Option<Request> {
        if ![guild, user, role].into_iter().all(snowflake) {
            return None;
        }
        let method = if give { Method::Put } else { Method::Delete };
        Some(
            self.bare(
                method,
                &format!("/guilds/{guild}/members/{user}/roles/{role}"),
            )?
            .header(
                "X-Audit-Log-Reason",
                Text::plain(self.interactions.wording().text(if give {
                    Phrase::AuditSubscribed
                } else {
                    Phrase::AuditUnsubscribed
                })),
            ),
        )
    }

    /// The request listing the roles of `guild`; none without a bot token or
    /// where the id is not written as Discord writes one.
    #[must_use]
    pub fn roles(&self, guild: &str) -> Option<Request> {
        if !snowflake(guild) {
            return None;
        }
        self.bare(Method::Get, &format!("/guilds/{guild}/roles"))
    }

    /// The request that makes a mentionable role named `name` in `guild`;
    /// none without a bot token or where the id is not written as Discord
    /// writes one.
    #[must_use]
    pub fn make_role(&self, guild: &str, name: &str) -> Option<Request> {
        if !snowflake(guild) {
            return None;
        }
        self.json(
            Method::Post,
            &format!("/guilds/{guild}/roles"),
            &json!({ "name": name, "mentionable": true }),
        )
    }
}

/// The roles a `GET /guilds/{guild}/roles` answer lists, each as its id and
/// name.
///
/// # Errors
///
/// What Discord said, where it did not list them.
pub fn listed(response: &Response) -> Result<Vec<(String, String)>, String> {
    took(response)?;
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    Ok(body
        .as_array()
        .map(|roles| {
            roles
                .iter()
                .filter_map(|role| {
                    Some((text(role, "id")?.to_owned(), text(role, "name")?.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// The role a `POST /guilds/{guild}/roles` answer made, as its id and name.
///
/// # Errors
///
/// What Discord said, where it made none.
pub fn made(response: &Response) -> Result<(String, String), String> {
    took(response)?;
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    match (text(&body, "id"), text(&body, "name")) {
        (Some(id), Some(name)) => Ok((id.to_owned(), name.to_owned())),
        _ => Err("Discord answered with no role".to_owned()),
    }
}

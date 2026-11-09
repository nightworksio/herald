//! Setting a Discord destination up to take commands: its commands
//! registered and its opt-in roles listed or made (ADR-0016).

use herald_ports::{Request, Response, Secret};

use crate::interact::{Interacting, listed, made, took};
use crate::interactions::Interactions;
use crate::options::DestinationOptions;

/// What setting a Discord destination up asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task<'a> {
    /// Register its commands.
    Commands,
    /// List the server `guild`'s roles, or make one named `create`.
    Roles {
        /// The server.
        guild: &'a str,
        /// The name of a role to make.
        create: Option<&'a str>,
    },
}

/// The base URL `task` is asked at and its request, of the destination
/// `options`, which takes commands as `interactions`, asked as its bot
/// `bot`.
///
/// # Errors
///
/// Where an id it names is not written as Discord writes one.
pub fn request(
    options: &DestinationOptions,
    interactions: &Interactions,
    bot: Secret,
    task: Task<'_>,
) -> Result<(String, Request), String> {
    let interacting = Interacting::new(interactions.clone(), Some(bot), options.api.as_deref());
    let request = match task {
        Task::Commands => interacting.register(),
        Task::Roles {
            guild,
            create: Some(role),
        } => interacting.make_role(guild, role),
        Task::Roles {
            guild,
            create: None,
        } => interacting.roles(guild),
    }
    .ok_or_else(|| "the server's id is not written as Discord writes one".to_owned())?;
    Ok((interacting.api().to_owned(), request))
}

/// What is said of `task` done for a destination taking commands as
/// `interactions`, from Discord's `response`.
///
/// # Errors
///
/// What Discord said where it did not do it.
pub fn said(
    interactions: &Interactions,
    task: Task<'_>,
    response: &Response,
) -> Result<Vec<String>, String> {
    Ok(match task {
        Task::Commands => {
            took(response)?;
            let registered: Vec<&str> = interactions
                .commands
                .iter()
                .map(|command| command.name())
                .collect();
            let place = interactions.guild.as_ref().map_or_else(
                || "in every server the application is in".to_owned(),
                |guild| format!("in the server {guild}"),
            );
            vec![format!(
                "Registered {} with Discord {place}.",
                registered.join(", ")
            )]
        }
        Task::Roles {
            create: Some(_), ..
        } => {
            let (id, made) = made(response)?;
            vec![
                format!("Made the role {made}. Offer it on a route with:"),
                format!("subscribe = {{ role = \"{id}\" }}"),
            ]
        }
        Task::Roles { create: None, .. } => listed(response)?
            .into_iter()
            .map(|(id, role)| format!("{id}  {role}"))
            .collect(),
    })
}

//! Setting a destination up to take commands (ADR-0016): Discord's commands
//! registered and its opt-in roles made, Slack's app manifest, and the
//! registration a Matrix homeserver installs.

use std::collections::BTreeMap;
use std::io::Write;

use herald_config::{Config, Destination};
use herald_core::{DestinationName, SecretRef};
use herald_native::EnvSecrets;
use herald_ports::{Http, Request, Response, Secret, Secrets};

use super::output::say;
use super::{Connect, DONE};

/// The destination of `kind` named `named`, or the only one of that kind
/// that takes commands where none is named.
fn chosen<'a>(
    config: &'a Config,
    kind: &str,
    named: Option<&str>,
) -> Result<(DestinationName, &'a Destination), String> {
    let taking: Vec<(&DestinationName, &Destination)> = config
        .destinations
        .iter()
        .filter(|(_, destination)| destination.kind() == kind && destination.answering().is_some())
        .collect();
    let listed = || {
        taking
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (named, taking.as_slice()) {
        (_, []) => Err(format!(
            "no {kind} destination takes commands; give one [destinations.<name>.interactions]"
        )),
        (None, [(name, destination)]) => Ok(((*name).clone(), destination)),
        (None, _) => Err(format!(
            "more than one {kind} destination takes commands; name one of {} with --destination",
            listed()
        )),
        (Some(named), _) => taking
            .iter()
            .find(|(name, _)| name.as_str() == named)
            .map(|(name, destination)| ((*name).clone(), *destination))
            .ok_or_else(|| {
                format!(
                    "no {kind} destination named \"{named}\" takes commands; the ones that do are {}",
                    listed()
                )
            }),
    }
}

async fn secret(vars: &BTreeMap<String, String>, named: &SecretRef) -> Result<Secret, String> {
    EnvSecrets::from_vars(vars.clone())
        .secret(&named.secret)
        .await
        .map_err(|error| error.to_string())
}

async fn sent(connect: Connect<'_>, base: &str, request: Request) -> Result<Response, String> {
    let http = connect(base)?;
    http.send(request).await.map_err(|error| error.to_string())
}

/// What `herald discord …` is asked.
pub(crate) enum Discording<'a> {
    /// Register the commands.
    Commands,
    /// List the server's roles, or make one named `create`.
    Roles {
        /// The server.
        guild: &'a str,
        /// The name of a role to make.
        create: Option<&'a str>,
    },
}

/// The Discord destination `named`, set up as `asked`.
pub(crate) async fn discord(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    named: Option<&str>,
    asked: Discording<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (name, destination) = chosen(config, Destination::DISCORD, named)?;
    let Destination::Discord(options) = destination else {
        return Err(format!("{} is not a Discord destination", name.as_str()));
    };
    let (Some(interactions), Some(token)) = (&options.interactions, &options.token) else {
        return Err(format!(
            "{} has no bot token, which Discord's commands and roles are set up with",
            name.as_str()
        ));
    };
    let bot = secret(vars, token).await?;
    let interacting = herald_dest_discord::Interacting::new(
        interactions.clone(),
        Some(bot),
        options.api.as_deref(),
    );
    let api = interacting.api().to_owned();
    let unwritten = || "the server's id is not written as Discord writes one".to_owned();
    match asked {
        Discording::Commands => {
            let request = interacting.register().ok_or_else(unwritten)?;
            herald_dest_discord::took(&sent(connect, &api, request).await?)?;
            let registered: Vec<&str> = interactions
                .commands
                .iter()
                .map(|command| command.name())
                .collect();
            let place = interactions.guild.as_ref().map_or_else(
                || "in every server the application is in".to_owned(),
                |guild| format!("in the server {guild}"),
            );
            say(
                out,
                &format!("Registered {} with Discord {place}.", registered.join(", ")),
            );
        }
        Discording::Roles {
            guild,
            create: Some(role),
        } => {
            let request = interacting.make_role(guild, role).ok_or_else(unwritten)?;
            let (id, made) = herald_dest_discord::made(&sent(connect, &api, request).await?)?;
            say(
                out,
                &format!("Made the role {made}. Offer it on a route with:"),
            );
            say(out, &format!("subscribe = {{ role = \"{id}\" }}"));
        }
        Discording::Roles {
            guild,
            create: None,
        } => {
            let request = interacting.roles(guild).ok_or_else(unwritten)?;
            for (id, role) in herald_dest_discord::listed(&sent(connect, &api, request).await?)? {
                say(out, &format!("{id}  {role}"));
            }
        }
    }
    Ok(DONE)
}

/// The app manifest of the Slack destination `named`, its commands posted to
/// herald at `url` or carried over Socket Mode.
pub(crate) fn slack(
    config: &Config,
    named: Option<&str>,
    url: Option<&str>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (name, destination) = chosen(config, Destination::SLACK, named)?;
    let Destination::Slack(options) = destination else {
        return Err(format!("{} is not a Slack destination", name.as_str()));
    };
    let Some(interactions) = &options.interactions else {
        return Err(format!("{} takes no commands", name.as_str()));
    };
    let posted = interactions.via == herald_dest_slack::Via::Http;
    let url = match (posted, url) {
        (true, None) => {
            return Err(
                "Slack posts commands to herald's address; give it with --url, such as --url https://herald.example".to_owned(),
            );
        }
        (true, Some(url)) => Some(web(url)?),
        (false, _) => None,
    };
    let interacting =
        herald_dest_slack::Interacting::new(interactions.clone(), options.api.as_deref());
    say(out, &interacting.manifest(name.as_str(), url));
    let kept = if posted {
        interactions
            .signing
            .as_ref()
            .map(|named| format!("its signing secret as {}", named.secret))
    } else {
        interactions.app.as_ref().map(|named| {
            format!(
                "an app-level token with connections:write as {}",
                named.secret
            )
        })
    };
    say(
        out,
        &format!(
            "Make a Slack app from this manifest at https://api.slack.com/apps, install it, and keep {}.",
            kept.unwrap_or_default()
        ),
    );
    Ok(DONE)
}

/// The registration a homeserver installs for the Matrix destination
/// `named`, reaching herald at `url`, with two tokens made by `random`.
pub(crate) fn matrix(
    config: &Config,
    named: Option<&str>,
    url: &str,
    random: &dyn Fn() -> Option<String>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (name, destination) = chosen(config, Destination::MATRIX, named)?;
    let Destination::Matrix(options) = destination else {
        return Err(format!("{} is not a Matrix destination", name.as_str()));
    };
    let Some(interactions) = options
        .interactions
        .as_ref()
        .filter(|interactions| interactions.via == herald_dest_matrix::Via::Appservice)
    else {
        return Err(format!(
            "{} reads commands by syncing, so the homeserver installs nothing",
            name.as_str()
        ));
    };
    let url = web(url)?;
    let (Some(homeserver), Some(appservice)) = (random(), random()) else {
        return Err("the system's random generator failed, so no token was made".to_owned());
    };
    let interacting = herald_dest_matrix::Interacting::new(
        &options.homeserver,
        interactions.clone(),
        Secret::new("the application service's token", appservice.clone()),
    );
    let secret_of =
        |named: Option<&SecretRef>| named.map(|named| named.secret.clone()).unwrap_or_default();
    say(
        out,
        &format!(
            "Keep the homeserver's token as {} and the application service's token as {}, then give the homeserver's administrator this registration:",
            secret_of(interactions.homeserver_token.as_ref()),
            secret_of(interactions.appservice_token.as_ref())
        ),
    );
    let registration = interacting.registration(
        &format!("{url}/matrix/{}", name.as_str()),
        &homeserver,
        &appservice,
    );
    for line in registration.lines() {
        say(out, line);
    }
    say(out, "herald prints these tokens only this once.");
    Ok(DONE)
}

/// `url` with no trailing slash, where it is an `https` or `http` address
/// with nothing a YAML or JSON string could be broken by.
fn web(url: &str) -> Result<&str, String> {
    let sound = (url.starts_with("https://") || url.starts_with("http://"))
        && !url
            .contains(|character: char| character.is_whitespace() || "\"'\\".contains(character));
    if sound {
        Ok(url.trim_end_matches('/'))
    } else {
        Err(format!(
            "\"{url}\" is not an address herald is served at; give one such as https://herald.example"
        ))
    }
}

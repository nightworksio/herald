//! Setting a destination up to take commands (ADR-0016): Discord's commands
//! registered and its opt-in roles made, Slack's app manifest, and the
//! registration a Matrix homeserver installs.

use std::collections::BTreeMap;
use std::io::Write;

use herald_config::{Config, Destination};
use herald_core::{DestinationName, SecretRef};
use herald_environment::EnvSecrets;
use herald_ports::{Http, Request, Response, Secret, Secrets};

use super::output::say;
use super::{Connect, DONE};

/// What `taken` takes of the destination named `named` among those of
/// `kind` that take commands, or of the only one where none is named.
fn chosen<'a, T: Copy>(
    config: &'a Config,
    kind: &str,
    named: Option<&str>,
    taken: impl Fn(&'a Destination) -> Option<T>,
) -> Result<(DestinationName, T), String> {
    let taking: Vec<(&DestinationName, T)> = config
        .destinations
        .iter()
        .filter_map(|(name, destination)| Some((name, taken(destination)?)))
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
        (None, [(name, destination)]) => Ok(((*name).clone(), *destination)),
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
    let http = connect.reach(base)?;
    http.send(request).await.map_err(|error| error.to_string())
}

/// The Discord destination `named`, set up as `task` asks.
pub(crate) async fn discord(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    named: Option<&str>,
    task: herald_dest_discord::setup::Task<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (name, (options, interactions)) =
        chosen(config, herald_dest_discord::KIND, named, |destination| {
            let options = destination.of::<herald_dest_discord::DestinationOptions>()?;
            Some((options, options.interactions.as_ref()?))
        })?;
    let Some(token) = &options.token else {
        return Err(format!(
            "{} has no bot token, which Discord's commands and roles are set up with",
            name.as_str()
        ));
    };
    let bot = secret(vars, token).await?;
    let (api, request) = herald_dest_discord::setup::request(options, interactions, bot, task)?;
    let response = sent(connect, &api, request).await?;
    for line in herald_dest_discord::setup::said(interactions, task, &response)? {
        say(out, &line);
    }
    Ok(DONE)
}

/// The app manifest of the Slack destination `named`, its commands posted to
/// herald at `url`, under the config's `[server] base`, or carried over
/// Socket Mode.
pub(crate) fn slack(
    config: &Config,
    named: Option<&str>,
    url: Option<&str>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (name, (options, interactions)) =
        chosen(config, herald_dest_slack::KIND, named, |destination| {
            let options = destination.of::<herald_dest_slack::DestinationOptions>()?;
            Some((options, options.interactions.as_ref()?))
        })?;
    let url = match url {
        Some(url) if herald_dest_slack::setup::posted(interactions) => {
            Some(format!("{}{}", web(url)?, config.server.base))
        }
        _ => None,
    };
    for line in
        herald_dest_slack::setup::manifest(name.as_str(), options, interactions, url.as_deref())?
    {
        say(out, &line);
    }
    Ok(DONE)
}

/// The registration a homeserver installs for the Matrix destination
/// `named`, reaching herald at `url` under the config's `[server] base`, with
/// two tokens made by `random`.
pub(crate) fn matrix(
    config: &Config,
    named: Option<&str>,
    url: &str,
    random: &dyn Fn() -> Option<String>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (name, (options, interactions)) =
        chosen(config, herald_dest_matrix::KIND, named, |destination| {
            let options = destination.of::<herald_dest_matrix::DestinationOptions>()?;
            Some((options, options.interactions.as_ref()?))
        })?;
    herald_dest_matrix::setup::installs(name.as_str(), interactions)?;
    let url = format!("{}{}", web(url)?, config.server.base);
    let (Some(homeserver), Some(appservice)) = (random(), random()) else {
        return Err("the system's random generator failed, so no token was made".to_owned());
    };
    for line in herald_dest_matrix::setup::registration(
        name.as_str(),
        options,
        interactions,
        &url,
        (&homeserver, &appservice),
    )? {
        say(out, &line);
    }
    Ok(DONE)
}

/// `url` with no trailing slash, where it is an `https` or `http` address
/// with nothing a YAML or JSON string could be broken by.
fn web(url: &str) -> Result<&str, String> {
    let sound = herald_core::web_url(url)
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

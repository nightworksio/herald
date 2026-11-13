//! Setting a destination up to take commands (ADR-0016): Discord's commands
//! registered and its opt-in roles made, Slack's app manifest, and the
//! registration a Matrix homeserver installs, each as its destination's
//! setup says.

use std::collections::BTreeMap;
use std::io::Write;

use herald_config::Config;
use herald_core::DestinationName;
use herald_environment::EnvSecrets;
use herald_kinds::{Made, Served, Setup, SetupStep, SetupTask};
use herald_ports::{Http, Request, Response, Secrets};

use super::cli::{Discording, Matrixing, Setting, Slacking};
use super::output::say;
use super::{Connect, DONE};

/// Why a setup that hands out new tokens could not.
const UNRANDOM: &str = "the system's random generator failed, so no token was made";

/// The setup of the destination named `named` among those of `kind` that
/// take commands, or of the only one where none is named.
fn chosen<'a>(
    config: &'a Config,
    kind: &str,
    named: Option<&str>,
) -> Result<(&'a DestinationName, Box<dyn Setup + 'a>), String> {
    let mut taking: Vec<(&DestinationName, Box<dyn Setup + 'a>)> = config
        .destinations
        .iter()
        .filter(|(_, destination)| destination.kind() == kind)
        .filter_map(|(name, destination)| Some((name, destination.setup()?)))
        .collect();
    let listed = taking
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let found = match named {
        Some(named) => taking.iter().position(|(name, _)| name.as_str() == named),
        None if taking.len() == 1 => Some(0),
        None => None,
    };
    match (found, named) {
        (Some(index), _) => Ok(taking.swap_remove(index)),
        _ if taking.is_empty() => Err(format!(
            "no {kind} destination takes commands; give one [destinations.<name>.interactions]"
        )),
        (None, None) => Err(format!(
            "more than one {kind} destination takes commands; name one of {listed} with --destination"
        )),
        (None, Some(named)) => Err(format!(
            "no {kind} destination named \"{named}\" takes commands; the ones that do are {listed}"
        )),
    }
}

async fn sent(connect: Connect<'_>, base: &str, request: Request) -> Result<Response, String> {
    let http = connect.reach(base)?;
    http.send(request).await.map_err(|error| error.to_string())
}

/// The destination `setting` names set up as it asks, its secrets read from
/// `vars`, its requests sent through `connect`, and each new token made by
/// `random`.
pub(crate) async fn set_up(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    setting: &Setting,
    random: &dyn Fn() -> Option<String>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let base = config.server.base.as_str();
    let (kind, named, task) = match setting {
        Setting::Discord(Discording::Commands { destination }) => {
            (herald_dest_discord::KIND, destination, SetupTask::Commands)
        }
        Setting::Discord(Discording::Roles {
            destination,
            guild,
            create,
        }) => (
            herald_dest_discord::KIND,
            destination,
            SetupTask::Roles {
                guild,
                create: create.as_deref(),
            },
        ),
        Setting::Slack(Slacking::Manifest { destination, url }) => (
            herald_dest_slack::KIND,
            destination,
            SetupTask::Manifest {
                served: url.as_deref().map(|url| Served { url, base }),
            },
        ),
        Setting::Matrix(Matrixing::Registration { destination, url }) => (
            herald_dest_matrix::KIND,
            destination,
            SetupTask::Registration {
                served: Served { url, base },
            },
        ),
    };
    let (name, setup) = chosen(config, kind, named.as_deref())?;
    let name = name.as_str();
    let secrets = EnvSecrets::from_vars(vars.clone());
    let needs = setup.needs(name, task)?;
    let mut read = Vec::new();
    for named in needs.secrets {
        read.push(
            secrets
                .secret(&named.secret)
                .await
                .map_err(|error| error.to_string())?,
        );
    }
    let tokens = (0..needs.tokens)
        .map(|_| random())
        .collect::<Option<Vec<String>>>()
        .ok_or_else(|| UNRANDOM.to_owned())?;
    let made = Made {
        secrets: read,
        tokens,
    };
    for step in setup.steps(name, task, made)? {
        let said = match step {
            SetupStep::Say(said) => said,
            SetupStep::Send { base, request } => {
                setup.said(task, &sent(connect, &base, request).await?)?
            }
        };
        for line in said {
            say(out, &line);
        }
    }
    Ok(DONE)
}

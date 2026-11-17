//! Setting a source up: a webhook registered on each member of a GitLab
//! owner whose group cannot send its own (ADR-0041).

use std::collections::BTreeMap;
use std::io::Write;

use herald_config::Config;
use herald_core::SourceName;
use herald_environment::EnvSecrets;
use herald_kinds::{Credential, Polling, Registered, Registering, Unlisted, Unpolled};
use herald_ports::Secrets;

use super::output::say;
use super::{Connect, DONE, STOPPED};

/// Where a source's webhooks are delivered, after herald's address.
const HOOKS: &str = "/hooks/";

/// A webhook registered on each member the source `name` takes, pointing
/// at `url` or else `[server] url`, its secrets read from `vars` and its
/// requests sent through `connect`; each member said as it was found or
/// given one.
///
/// # Errors
///
/// Where the source registers no webhooks, herald's address is not given,
/// a secret cannot be read, or the owner cannot be listed.
pub(crate) async fn set_up_source(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    (name, url): (&str, Option<String>),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let source = config
        .sources
        .get(&SourceName::new(name))
        .ok_or_else(|| format!("no source is named \"{name}\""))?;
    let secrets = EnvSecrets::from_vars(vars.clone());
    let token = match source.token() {
        Some(named) => Some(Credential::Named(
            secrets
                .secret(&named.secret)
                .await
                .map_err(|error| error.to_string())?,
        )),
        None => None,
    };
    let (Registering::Setup | Registering::Round, Some(named), Polling::Many(expand)) = (
        source.registering(),
        source.webhook(),
        source.poller(token, false),
    ) else {
        return Err(format!(
            "the source {name} registers no webhook on its members; set member_hooks = \"setup\" or \"round\""
        ));
    };
    let url = url.or_else(|| config.server.url.clone()).ok_or_else(|| {
        "herald's address is not given; pass --url, or give [server] url".to_owned()
    })?;
    let hook = format!("{}{HOOKS}{name}", url.trim_end_matches('/'));
    let webhook = secrets
        .secret(&named.secret)
        .await
        .map_err(|error| error.to_string())?;
    let http = connect.reach(expand.api())?;
    let members = expand
        .listed(&http, None)
        .await
        .map_err(|unlisted| match unlisted {
            Unlisted::Over(reason)
            | Unlisted::Failed(Unpolled::Unreachable(reason) | Unpolled::Unreadable(reason)) => {
                format!("the owner could not be listed: {reason}")
            }
        })?
        .members();
    let mut done = DONE;
    for member in &members {
        let said = match expand.register(&http, member, (&hook, &webhook)).await {
            Ok(Registered::Found) => "has a webhook to herald".to_owned(),
            Ok(Registered::Made) => "was given a webhook to herald".to_owned(),
            Err(Unpolled::Unreachable(reason) | Unpolled::Unreadable(reason)) => {
                done = STOPPED;
                format!("could not be given a webhook: {reason}")
            }
        };
        say(out, &format!("{}: {said}", member.path));
    }
    Ok(done)
}

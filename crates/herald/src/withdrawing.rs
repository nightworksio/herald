//! A release's posts withdrawn from the CLI, or what a withdrawal would
//! remove (ADR-0048).

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::told::{steps, why, withdrew};
use herald_compose::{Host, Hosting, ServingOn, Through};
use herald_config::{Config, Uses};
use herald_core::{Identity, RouteId};
use herald_flows::{Announced, Outcome, Stop, Withdrawable, withdraw, withdrawable};
use herald_ports::{Keyring, Store};

use super::output::say;
use super::stores::composed;
use super::{Connect, DONE, STOPPED, place};

/// What the operator asks to withdraw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Asked {
    route: Option<RouteId>,
    identity: Identity,
    dry_run: bool,
}

impl Asked {
    /// What `names` ask: a route and an identity, or an identity alone
    /// where `every_route` says so.
    ///
    /// # Errors
    ///
    /// Where `names` holds other than one name for every route, or two for
    /// one route.
    pub(crate) fn of(names: &[String], every_route: bool, dry_run: bool) -> Result<Self, String> {
        let (route, identity) = match (names, every_route) {
            ([identity], true) => (None, identity),
            ([route, identity], false) => (Some(RouteId::new(route)), identity),
            (_, true) => {
                return Err("--every-route takes the release's identity alone".to_owned());
            }
            (_, false) => {
                return Err(
                    "name the route and the release's identity, or give --every-route".to_owned(),
                );
            }
        };
        Ok(Self {
            route,
            identity: Identity::new(identity),
            dry_run,
        })
    }
}

/// What was withdrawn, or would be.
pub(crate) enum Done {
    Withdrawn(Vec<Announced>),
    Planned(Result<Vec<Withdrawable>, Stop>),
}

impl<H: Host> Through<H> for Asked {
    type Done = Done;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Done {
        let target = (self.route.as_ref(), &self.identity);
        let (config, connected, ports) = (serving.config, serving.connected, serving.ports);
        if self.dry_run {
            Done::Planned(withdrawable(config, connected, ports, target).await)
        } else {
            Done::Withdrawn(withdraw(config, connected, ports, target, run).await)
        }
    }
}

/// Answers `asked`, saying what each route withdrew or would withdraw.
///
/// # Errors
///
/// When the store or the routes cannot be reached.
pub(crate) async fn withdrawing(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    asked: Asked,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Announces,
        place(vars),
    )
    .await?;
    let identity = asked.identity.clone();
    let done = composed.run(asked).await;
    Ok(match done {
        Done::Withdrawn(withdrawn) => {
            let code = reported(&identity, &withdrawn, out);
            code.max(crate::pages::logged(&composed, out).await)
        }
        Done::Planned(Ok(planned)) => {
            for line in planned.iter().flat_map(|route| plan(&identity, route)) {
                say(out, &line);
            }
            DONE
        }
        Done::Planned(Err(stop)) => {
            say(out, &format!("nothing was read: {}", why(&stop)));
            STOPPED
        }
    })
}

/// What each route did withdrawing `identity`, and the code it ends with:
/// one where a route stopped, or no route is named.
fn reported(identity: &Identity, withdrawn: &[Announced], out: &mut dyn Write) -> u8 {
    if withdrawn.is_empty() {
        say(out, "The config names no such route.");
        return STOPPED;
    }
    let mut code = DONE;
    for announced in withdrawn {
        if matches!(
            announced.outcome,
            Outcome::Stopped { .. } | Outcome::Held { .. }
        ) {
            code = STOPPED;
        }
        say(out, &withdrew(identity, announced));
    }
    code
}

/// What `route` would withdraw of `identity`, in words.
fn plan(identity: &Identity, route: &Withdrawable) -> Vec<String> {
    let name = route.route.as_str();
    if let Some(kept) = &route.kept {
        return vec![format!("{name}: keeps {}, as {kept}", identity.as_str())];
    }
    if route.posts.is_empty() {
        return vec![format!(
            "{name}: announced nothing of {}",
            identity.as_str()
        )];
    }
    route
        .posts
        .iter()
        .map(|post| {
            let posted = post.identity.as_str();
            match &post.group {
                Some(group) => format!("{name}: would take {posted} out of the group {group}"),
                None if post.steps.is_empty() => {
                    format!("{name}: would mark {posted} withdrawn; nothing is left to delete")
                }
                None => format!("{name}: would withdraw {posted} ({})", steps(&post.steps)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;

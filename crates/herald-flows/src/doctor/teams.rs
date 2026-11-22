//! What doctor finds online of each source reading its forge's own teams:
//! a forge that refuses them, named with the permission its token needs,
//! and one whose rate limit was reached, which stops the read (ADR-0077).
//! No team, member or figure is said, since doctor's output is
//! no private surface.

use herald_config::Config;
use herald_kinds::{Teamed, Unpolled};
use herald_ports::{DynHttp, Http};

use super::{Findings, gap};
use crate::Connected;

/// What each source of `config` reading its forge's teams finds asking
/// for them through `connected`: a gap where the forge refuses them or
/// cannot be reached, its teams read and dropped.
pub async fn forge_teams<H: Http>(config: &Config, connected: &Connected<H>) -> Findings {
    let mut findings = Findings::default();
    let reached = connected.sources.iter().filter_map(|(prepared, http)| {
        let source = config.sources.get(&prepared.name)?;
        source
            .forge_teams()
            .then_some((prepared.name.as_str(), prepared.queue.as_ref()?, http))
    });
    for (name, queue, http) in reached {
        let why = match queue.teams(http as &dyn DynHttp).await {
            Ok(Teamed::Read(_)) => {
                findings.notes.push(format!(
                    "The source {name} reads its forge's teams for each view showing its load."
                ));
                continue;
            }
            Ok(Teamed::Refused(permission)) => format!(
                "the forge refused to list the teams of the organization or group owning its repositories, or their members, or the owner is no organization or group; its token needs {permission}. Until then each view's load counts the config's teams alone"
            ),
            Err(Unpolled::RateLimited(_)) => "the forge's rate limit was reached reading its teams, so a view showing its load stops where it reads them until the limit resets, and never counts the config's teams in their place".to_owned(),
            Err(Unpolled::Unreachable(_) | Unpolled::Unreadable(_)) => "the forge's teams could not be read, so a view showing its load stops where it reads them".to_owned(),
        };
        findings.gaps.push(gap(
            &format!("reading the teams of the source \"{name}\""),
            format!("sources.{name}.teams"),
            why,
        ));
    }
    findings
}

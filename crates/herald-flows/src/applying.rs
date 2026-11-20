//! `herald rerun`: each rule's plans to re-run a flaky check, listed, or
//! applied by an operator signed in at the forge, each job re-run at most
//! once, with the source's write credential, after the checks made right
//! before the call (ADR-0068).

use std::collections::{BTreeMap, BTreeSet};

use herald_config::{Config, Rule};
use herald_core::{Rerun, SourceName, grouped};
use herald_kinds::UNWRITTEN;
use herald_kinds::{Credential, Source};
use herald_ports::{Base, Clock, DynSecrets, Http, Secret, Signer, Store, Timer};
use jiff::SignedDuration;

use crate::announce::{Ports, Stop};
use crate::deciding::decided_rows;
use crate::prepare::Connected;
use crate::report::{read_pulls, sources};

mod called;
mod signing;
mod writing;

pub use called::{Applied, Done};
pub use signing::UNFINISHED;
pub use writing::{SHARED_INSTALLATION, UNSEPARATED};

/// Why an apply is refused where the store keeps no record of a re-run.
pub const UNKEPT: &str = "the store keeps no record of a re-run, so herald cannot make sure it re-runs a job once; choose a store that keeps records";

/// Why an apply is refused where the operator named no job.
pub const UNCHOSEN: &str = "name each job to re-run as the plan prints it, --job <job>@<head>; list the plan with herald rerun --plan";

/// Why a job named is not re-run where today's plan does not hold it.
pub const UNPLANNED: &str = "the plan no longer holds this job on this head: the head moved, the job is no longer failing, or the check is no longer flaky; list the plan again";

/// One job a rule's plan would re-run, or why it would re-run none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// The rule.
    pub rule: String,
    /// The source that reads the pull request's repository.
    pub source: SourceName,
    /// The repository's full path.
    pub repository: String,
    /// The pull request's number.
    pub number: u64,
    /// The flaky check failing on it.
    pub check: String,
    /// How many times the check flipped within the view's window.
    pub flips: usize,
    /// The head commit it failed on.
    pub head: String,
    /// The job and its run, or why none can be re-run.
    pub run: Result<Rerun, String>,
}

/// What signing in shows the operator: where to go, and the code to enter.
#[derive(Debug, Clone, Copy)]
pub struct SigningIn<'a> {
    /// The source whose forge the operator signs in at.
    pub source: &'a str,
    /// The page the code is entered at.
    pub verification: &'a str,
    /// The code.
    pub user_code: &'a str,
    /// How long it lasts.
    pub expires: SignedDuration,
}

/// What an apply reaches beside a run's own connections: the host's
/// secrets, a connection made for each base, the time between asks of the
/// device flow, and where the operator is shown how to sign in.
pub struct Applying<'a, H, T> {
    /// The host's secrets, for the write credential alone.
    pub secrets: &'a dyn DynSecrets,
    /// A connection reaching only `base`.
    pub connect: &'a (dyn Fn(Base<'_>) -> Result<H, String> + Sync),
    /// What waits between asks.
    pub timer: &'a T,
    /// Where the operator is shown how to sign in.
    pub told: &'a dyn Fn(SigningIn<'_>),
}

/// The rules `rule` names: that one, where it plans re-runs, or every rule
/// that does.
fn ruled<'c>(config: &'c Config, rule: Option<&str>) -> Result<Vec<(&'c str, &'c Rule)>, Stop> {
    match rule {
        Some(name) => match config.rules.get_key_value(name) {
            Some((name, rule)) if rule.rerun => Ok(vec![(name.as_str(), rule)]),
            Some(_) => Err(Stop::Source(format!(
                "the rule \"{name}\" plans no re-run; give it flaky = {{ rerun = true }}"
            ))),
            None => Err(Stop::Source(format!(
                "no rule is named \"{name}\"; the rules are {}",
                config
                    .rules
                    .keys()
                    .map(|name| format!("\"{name}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        },
        None => Ok(config
            .rules
            .iter()
            .filter(|(_, rule)| rule.rerun)
            .map(|(name, rule)| (name.as_str(), rule))
            .collect()),
    }
}

/// The plans of the rule `rule`, or of every rule that plans re-runs, read
/// now: no write credential is read, and nothing is announced.
///
/// # Errors
///
/// Where no such rule plans re-runs, or a view or a run could not be read.
pub async fn rerun_plans<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    rule: Option<&str>,
) -> Result<Vec<Planned>, Stop> {
    let (config, connected, ports) = flow;
    let rules = ruled(config, rule)?;
    crate::poll::minting(flow, |_| true).await;
    let mut planned = Vec::new();
    let viewed = rules
        .into_iter()
        .filter_map(|(name, rule)| Some((name, rule, config.views.get(&rule.watch.view)?)));
    for (name, rule, view) in viewed {
        let sources = sources(connected, view)?;
        let read = read_pulls((config, &sources, ports), view).await?;
        let shown = grouped(&view.view, decided_rows(flow, view, &read.pulls).await);
        for planning in crate::reruns::plans((connected, ports), rule, (view, &shown)).await? {
            for run in planning.runs() {
                planned.push(Planned {
                    rule: name.to_owned(),
                    source: planning.source.clone(),
                    repository: planning.pull.repository.clone(),
                    number: planning.pull.number,
                    check: planning.check.to_owned(),
                    flips: planning.flips,
                    head: planning.head.to_owned(),
                    run: run.cloned().map_err(ToOwned::to_owned),
                });
            }
        }
    }
    Ok(planned)
}

/// The read credential the source `name`, `source`, reads with: the
/// token it names, read through `secrets`, or its App's minted token.
async fn read_credential<H>(
    (source, connected): (&Source, &Connected<H>),
    secrets: &dyn DynSecrets,
    name: &SourceName,
) -> Result<Option<(Credential, Option<Secret>)>, String> {
    if let Some((minted, key)) = connected.mintings.iter().find_map(|(minting, _)| {
        minting
            .minted_for(name)
            .map(|minted| (minted, minting.key().clone()))
    }) {
        return Ok(Some((Credential::Minted(minted), Some(key))));
    }
    match source.token() {
        Some(named) => secrets
            .secret_boxed(&named.secret)
            .await
            .map(|secret| Some((Credential::Named(secret.clone()), Some(secret))))
            .map_err(|error| error.to_string()),
        None => Ok(None),
    }
}

/// Each job of `chosen`, given as `(job, head)`, that the plans of the rule
/// `rule` hold now, re-run once with its source's write credential: the
/// plans are read again, the operator signs in through `applying` at each
/// source of a job they hold alone, and each such job is checked right
/// before its call; each other refused, saying why. The operator's tokens
/// live until the apply ends, when each forge that lets a public client
/// revoke one is asked to, every token that arrived, whatever came after.
///
/// # Errors
///
/// Where no job is named, the store keeps no record of a re-run, or the
/// plans could not be read.
pub async fn rerun_apply<S: Store, C: Clock, G: Signer, H: Http, T: Timer>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    applying: &Applying<'_, H, T>,
    (rule, chosen): (Option<&str>, &[(String, String)]),
) -> Result<Vec<Applied>, Stop> {
    let (config, connected, ports) = flow;
    if chosen.is_empty() {
        return Err(Stop::Source(UNCHOSEN.to_owned()));
    }
    let Some(store) = ports.store.actions() else {
        return Err(Stop::Store(UNKEPT.to_owned()));
    };
    let planned = rerun_plans(flow, rule).await?;
    let found: Vec<Option<(&Planned, &Rerun)>> = chosen
        .iter()
        .map(|(job, head)| {
            planned.iter().find_map(|planned| match &planned.run {
                Ok(rerun) if rerun.job == *job && planned.head == *head => Some((planned, rerun)),
                _ => None,
            })
        })
        .collect();
    let sources: BTreeSet<&SourceName> = found
        .iter()
        .flatten()
        .map(|(planned, _)| &planned.source)
        .collect();
    let mut ready = BTreeMap::new();
    for (prepared, http) in &connected.sources {
        let name = &prepared.name;
        let Some(source) = config.sources.get(name).filter(|_| sources.contains(name)) else {
            continue;
        };
        let signed = writing::signed(flow, applying, (name, source, http)).await;
        ready.insert(name.clone(), signed);
    }
    let mut applied = Vec::new();
    for ((job, head), found) in chosen.iter().zip(found) {
        applied.push(match found {
            None => Applied::unplanned(job, head),
            Some((planned, rerun)) => match ready.get(&planned.source) {
                Some((Some(revoking), Ok(signed))) => {
                    let signed = (signed, &revoking.token, store);
                    called::called(signed, ports, (planned, rerun)).await
                }
                Some((_, Err(why))) => Applied::refused(planned, rerun, why),
                Some((None, Ok(_))) | None => Applied::refused(planned, rerun, UNWRITTEN),
            },
        });
    }
    for revoking in ready.into_values().filter_map(|(revoking, _)| revoking) {
        revoking.end().await;
    }
    Ok(applied)
}

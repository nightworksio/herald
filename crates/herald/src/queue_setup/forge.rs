//! What the forge says of a merge queue's branch and repository, as
//! `herald setup merge-queue` shows it, and the methods it offers by it
//! (ADR-0087).

use std::collections::BTreeMap;

use herald_compose::Hosting;
use herald_config::{Config, Uses};
use herald_core::Printed;
use herald_core::merging::{Method, Runs};
use herald_flows::BranchSaid;
use herald_kinds::Source;

use super::{asking_of, merged};
use crate::Connect;
use crate::asking::{Asking, choices, parsed};
use crate::stores::composed;

/// The methods offered on `source` in `repository`: each the config takes
/// that the forge does not refuse, narrowed to those `said` says the
/// repository allows where it read them whole, and with `runs = "forge"`
/// to the forge queue's own where it names one. An offer narrowed to none
/// is not narrowed.
pub(super) fn offered_methods(
    source: &Source,
    repository: Option<&str>,
    (runs, said): (Runs, Option<&BranchSaid>),
) -> Vec<(String, String)> {
    let taken: Vec<(String, String)> = choices::<Method>()
        .into_iter()
        .filter(|(name, _)| {
            parsed::<Method>(name).is_some_and(|method| {
                merged(source, &asking_of(repository, Some(method), false, None)).is_ok()
            })
        })
        .collect();
    let allowed = said
        .and_then(|said| said.methods.as_ref())
        .filter(|methods| methods.whole)
        .map(|methods| methods.allowed.as_slice());
    let own = said
        .and_then(|said| said.branched.queued.method)
        .filter(|_| runs == Runs::Forge);
    let narrowed: Vec<(String, String)> = taken
        .iter()
        .filter(|(name, _)| {
            parsed::<Method>(name).is_some_and(|method| {
                let one = allowed.and_then(|allowed| match allowed {
                    [one] => Some(*one),
                    _ => None,
                });
                let named = if method == Method::Repository {
                    one
                } else {
                    Some(method)
                };
                let allows = allowed
                    .is_none_or(|allowed| named.is_some_and(|named| allowed.contains(&named)));
                allows && own.is_none_or(|own| named == Some(own))
            })
        })
        .cloned()
        .collect();
    if narrowed.is_empty() { taken } else { narrowed }
}

/// What the forge says of `branch` of the merge queue `name` in
/// `candidate`, said: whether it has a merge queue there, by which method,
/// whether it merges only through it, and which methods the repository
/// allows, where read.
pub(super) async fn forge_said(
    (candidate, vars, connect): (&Config, &BTreeMap<String, String>, Connect<'_>),
    (name, branch, printed): (&str, &str, Printed),
    asking: &mut Asking<'_>,
) -> Option<BranchSaid> {
    let read = async {
        composed(
            candidate,
            vars,
            connect,
            Hosting::Actions,
            Uses::Polls,
            "setup merge-queue",
        )
        .await
        .map_err(Some)?
        .merge_branch((name, printed))
        .await
    }
    .await;
    let said = match read {
        Ok(said) => said,
        Err(why) => {
            asking.say(&why.filter(|_| printed.prints_failures()).map_or_else(
                || format!("{branch} was not read; --private, or [cli] private, prints why."),
                |why| format!("{branch} was not read: {why}"),
            ));
            return None;
        }
    };
    for line in said_lines(&said, branch) {
        asking.say(&line);
    }
    Some(said)
}

/// What `said` of `branch` is said as, a line each.
fn said_lines(said: &BranchSaid, branch: &str) -> Vec<String> {
    let queued = &said.branched.queued;
    let by = queued.method.map_or_else(String::new, |method| {
        format!(", merging by \"{}\"", method.name())
    });
    let mut lines = vec![match (said.branched.queue, queued.unhanded.as_deref()) {
        (Some(false), Some(why)) => format!("The forge has no merge queue on {branch}: {why}."),
        (Some(false), None) => format!("The forge has no merge queue on {branch}."),
        (Some(true), _) => format!("The forge has a merge queue on {branch}{by}."),
        (None, _) => format!("The forge does not say whether {branch} has a merge queue."),
    }];
    if let (Some(true), Some(why)) = (said.branched.queue, queued.unhanded.as_deref()) {
        lines.push(format!("{why}."));
    }
    if queued.only {
        lines.push(format!(
            "The forge merges into {branch} through its queue alone, so it refuses herald's own merge."
        ));
    }
    lines.push(
        match said.methods.as_ref().filter(|methods| methods.whole) {
            Some(methods) => format!(
                "The repository allows a merge by {}.",
                methods
                    .allowed
                    .iter()
                    .map(|method| format!("\"{}\"", method.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => "The forge does not say which methods the repository allows.".to_owned(),
        },
    );
    lines
}

#[cfg(test)]
mod tests;

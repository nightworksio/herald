//! GitLab's merge trains: where each open merge request of a project
//! stands in its target branch's train (ADR-0078).

use std::collections::BTreeMap;

use herald_core::status::FORBIDDEN;
use herald_core::{Pull, QueueEntry, QueueState};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _};
use jiff::Timestamp;
use serde::Deserialize;

use super::PAGE;
use crate::forge::Origin;
use crate::limited::{checked, limited};
use crate::{Forge, Token};

/// What a project's merge trains are read as.
pub(crate) const WHAT: &str = "merge train listing";

/// The path, under a project, of the first page of its active merge train
/// cars, the oldest first: at most [`PAGE`] are read.
fn path() -> String {
    format!("/merge_trains?scope=active&sort=asc&per_page={PAGE}")
}

#[derive(Deserialize)]
struct Request {
    iid: u64,
}

#[derive(Deserialize)]
struct Car {
    merge_request: Request,
    status: String,
    target_branch: String,
    created_at: Timestamp,
}

/// The state GitLab's train car `status` is.
fn state(status: &str) -> QueueState {
    match status {
        "fresh" => QueueState::Checking,
        "merging" => QueueState::Merging,
        _ => QueueState::Queued,
    }
}

/// Each merge request `body` names, by its number, with its entry: its
/// place in its target branch's train, counting from 1, its state and
/// when it joined; parsing no more than `parse` bytes.
fn cars(body: &[u8], parse: usize) -> Result<BTreeMap<u64, QueueEntry>, herald_wire::SourceError> {
    let cars: Vec<Car> = herald_wire::read(WHAT, parse, body)?;
    let mut places: BTreeMap<String, u64> = BTreeMap::new();
    let mut entries = BTreeMap::new();
    for car in cars {
        let place = places.entry(car.target_branch).or_default();
        *place += 1;
        let entry = QueueEntry {
            position: Some(*place),
            state: state(&car.status),
            since: Some(car.created_at),
            estimate: None,
            left: None,
        };
        entries.insert(car.merge_request.iid, entry);
    }
    Ok(entries)
}

/// The merge train entries of the project `origin` names, read with
/// `token`: none where GitLab refuses the read other than for its rate
/// limit, as it does without Premium or the permission to read them.
///
/// # Errors
///
/// Where the request could not be made or answered, its rate limit was
/// reached, or the answer could not be read.
async fn entries(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
) -> Result<BTreeMap<u64, QueueEntry>, Unpolled> {
    let request = origin.get(&path(), token)?;
    let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
    if answer.status == FORBIDDEN && limited(Forge::GitLab, WHAT, &answer).is_none() {
        return Ok(BTreeMap::new());
    }
    checked(Forge::GitLab, WHAT, &answer)?;
    cars(&answer.body, origin.reading().parse).map_err(Unpolled::unreadable)
}

/// `pulls`, each of the project `origin` names, with its merge train
/// entry where it rides a train.
///
/// # Errors
///
/// As [`entries`].
pub(crate) async fn trained<'p>(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
    pulls: impl IntoIterator<Item = &'p mut Pull>,
) -> Result<(), Unpolled> {
    let mut entries = entries(http, (origin, token)).await?;
    for pull in pulls {
        pull.queue = entries.remove(&pull.number);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

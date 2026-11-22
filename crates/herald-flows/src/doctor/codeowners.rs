//! What doctor finds online in the CODEOWNERS file each source reading
//! areas from one reads: each line herald skips, by number and why, a
//! file it reads none of (ADR-0073), and a file the store keeps no record
//! of, so that it is read in full every round (ADR-0074, ADR-0076).

use herald_config::Config;
use herald_core::{CodeownersKey, KeptCodeowners, SourceName};
use herald_kinds::{Codeowning, Found, Unpolled};
use herald_ports::{DynHttp, Http};
use jiff::Timestamp;

use super::Findings;
use crate::Connected;

/// The most skipped lines of one file doctor names, the rest counted.
pub const MOST_SKIPPED_NAMED: usize = 20;

/// What each source of `config` reading areas from CODEOWNERS finds in
/// the file of the one repository it reads, at its default branch,
/// through `connected` at `at`; a source reading an owner's repositories
/// reads each member's file only as a view reads it.
pub async fn codeowners<H: Http>(
    config: &Config,
    connected: &Connected<H>,
    at: Timestamp,
) -> Findings {
    let keeps = config.store.keeps_pulls();
    let mut findings = Findings::default();
    let reached = connected.sources.iter().filter_map(|(prepared, http)| {
        let source = config.sources.get(&prepared.name)?;
        Some((
            prepared.name.as_str(),
            source,
            prepared.queue.as_ref()?,
            http,
        ))
    });
    for (name, source, queue, http) in reached {
        let Some(repository) = queue.repository() else {
            if source.codeowned() {
                findings.notes.push(format!(
                    "The source {name} reads an owner's repositories, so doctor reads none of their CODEOWNERS files: a line one skips leaves its owners out of that repository's areas."
                ));
            }
            continue;
        };
        let Some(codeowning) = source.codeowning(repository) else {
            continue;
        };
        let read = queue
            .codeowners(
                http as &dyn DynHttp,
                (repository, None),
                (&codeowning, None),
            )
            .await;
        let key = CodeownersKey {
            source: SourceName::new(name),
            repository: repository.to_lowercase(),
            reference: None,
        };
        let unkept = match &read {
            Ok(Some(found)) if keeps => unkept(name, (key, found), (&codeowning, at)),
            _ => None,
        };
        findings.notes.extend(said(name, read, &codeowning.paths));
        findings.notes.extend(unkept);
    }
    findings
}

/// What doctor says of the source `name`'s CODEOWNERS file `found`, kept
/// under `key`, where the store keeps no record of it, read under
/// `codeowning` at `at`.
fn unkept(
    name: &str,
    (key, found): (CodeownersKey, &Found),
    (codeowning, at): (&Codeowning, Timestamp),
) -> Option<String> {
    let path = found.path.as_str();
    if found.file.refused().is_some() {
        return None;
    }
    let Some(etag) = found.etag.as_deref() else {
        return Some(format!(
            "The source {name}'s forge gives its CODEOWNERS file {path} no validator, so the store keeps no record of it and it is read in full every round."
        ));
    };
    let read = (codeowning.bounds(), at);
    KeptCodeowners::of(key, (path, etag), &found.file, read)
        .is_none()
        .then(|| {
            format!(
                "The store keeps no record of the source {name}'s CODEOWNERS file {path}, so it is read in full every round: the record would be longer than its most_codeowners of {} bytes.",
                codeowning.most
            )
        })
}

/// What doctor says of the source `name`'s CODEOWNERS file as `read`
/// found it, looked for at `paths`.
fn said(
    name: &str,
    read: Result<Option<herald_kinds::Found>, Unpolled>,
    paths: &[String],
) -> Vec<String> {
    let found = match read {
        Ok(Some(found)) => found,
        Ok(None) => {
            return vec![format!(
                "The source {name} finds no CODEOWNERS file at {} on the default branch, so its pull requests fall in no area.",
                paths.join(", ")
            )];
        }
        Err(Unpolled::Unreachable(_)) => {
            return vec![format!(
                "The source {name} could not read its CODEOWNERS file: its forge could not be reached."
            )];
        }
        Err(Unpolled::Unreadable(_)) => {
            return vec![format!(
                "The source {name} could not read its CODEOWNERS file: its forge's answer could not be read."
            )];
        }
        Err(Unpolled::RateLimited(_)) => {
            return vec![format!(
                "The source {name} could not read its CODEOWNERS file: its forge's rate limit was reached."
            )];
        }
    };
    let path = found.path.as_str();
    if let Some(why) = found.file.refused() {
        return vec![format!(
            "The source {name} reads none of its CODEOWNERS file {path}, so its pull requests fall in no area: {why}."
        )];
    }
    let skipped = found.file.skipped();
    let mut notes: Vec<String> = skipped
        .iter()
        .take(MOST_SKIPPED_NAMED)
        .map(|skipped| {
            format!(
                "The source {name} skips line {} of its CODEOWNERS file {path}, which matches no path: {}.",
                skipped.line, skipped.why
            )
        })
        .collect();
    if let Some(more) = skipped
        .len()
        .checked_sub(MOST_SKIPPED_NAMED)
        .filter(|more| *more > 0)
    {
        notes.push(format!(
            "The source {name} skips {more} more lines of its CODEOWNERS file {path}."
        ));
    }
    notes
}

#[cfg(test)]
mod tests;

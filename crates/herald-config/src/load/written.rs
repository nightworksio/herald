//! The write credential each source re-runs with, refused where it is the
//! credential the source reads with, by name or through the same GitHub
//! App's key, where herald reads its secret for anything else, or where it
//! is sent anywhere but the source's own API; and an operator's sign-in,
//! refused where it is a write App's own (ADR-0068).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{Address, SourceName};
use herald_kinds::{Source, WriteCredential};

use super::Loader;
use crate::Config;
use crate::config::GithubApp;
use crate::error::Errors;

/// The origin of `url`, where it is a web address.
fn origin(url: &str) -> Option<String> {
    Address::parse(url).map(|address| address.origin())
}

/// The secret a write credential is held in, as one apply reads it: a
/// token's, or a GitHub App's key, with where it is sent.
fn write_secret<'a>(
    write: &WriteCredential<'a>,
    apps: &'a BTreeMap<String, GithubApp>,
) -> Option<(&'a str, &'static str, Option<String>)> {
    match write {
        WriteCredential::Token { secret, api } => {
            Some((secret.secret.as_str(), "token", origin(api)))
        }
        WriteCredential::App(installed) => apps.get(installed.app).map(|app| {
            (
                app.private_key.secret.as_str(),
                "App's key",
                origin(&installed.api),
            )
        }),
    }
}

/// Every secret a source reads with: its token, and its App's key.
fn read_secrets<'a>(source: &'a Source, apps: &'a BTreeMap<String, GithubApp>) -> Vec<&'a str> {
    let app = source
        .installed()
        .and_then(|installed| apps.get(installed.app));
    source
        .token()
        .map(|token| token.secret.as_str())
        .into_iter()
        .chain(app.map(|app| app.private_key.secret.as_str()))
        .collect()
}

impl Loader<'_> {
    /// Each source of `sources` that names a write credential, refused
    /// where the credential is one it reads with, or an App of `apps` that
    /// is missing, is the App it reads as under another table, or mints
    /// at another origin than the source reads.
    pub(super) fn written(
        &mut self,
        (sources, spans): (
            &BTreeMap<SourceName, Source>,
            &BTreeMap<SourceName, Range<usize>>,
        ),
        apps: &BTreeMap<String, GithubApp>,
    ) {
        let issuers: BTreeSet<&str> = sources
            .values()
            .filter_map(|source| match source.write() {
                Some(WriteCredential::App(installed)) => apps.get(installed.app),
                _ => None,
            })
            .map(|app| app.issuer.as_str())
            .collect();
        for (name, source) in sources {
            if let Ok(signing) = source.signing()
                && issuers.contains(signing.client_id.as_str())
            {
                let span = spans.get(name).cloned().unwrap_or_default();
                self.error(
                    &span,
                    &format!("sources.{}.sign_in.client_id", name.as_str()),
                    format!(
                        "\"{}\" is the client id of a GitHub App a write names, so the operator's token could re-run what the App may; give the client id of an application that only reads",
                        signing.client_id
                    ),
                );
            }
        }
        for (name, source) in sources {
            let Some(write) = source.write() else {
                continue;
            };
            let span = spans.get(name).cloned().unwrap_or_default();
            let path = format!("sources.{}.write", name.as_str());
            let read = read_secrets(source, apps);
            let reading = source
                .installed()
                .and_then(|installed| apps.get(installed.app));
            let refusal = match write {
                WriteCredential::Token { secret, .. } if read.contains(&secret.secret.as_str()) => {
                    Some(format!(
                        "write.token names the secret {}, which the source reads with; give the write credential a secret of its own",
                        secret.secret
                    ))
                }
                WriteCredential::Token { .. } => None,
                WriteCredential::App(installed) => match apps.get(installed.app) {
                    None => Some(format!(
                        "no [github_apps.{}] is given; give it, or name one of {}",
                        installed.app,
                        super::list(&apps.keys().map(String::as_str).collect::<Vec<_>>())
                    )),
                    Some(app) if read.contains(&app.private_key.secret.as_str()) => Some(format!(
                        "the App {} signs with the secret {}, which the source reads with; re-runs are made as another App, with a key of its own",
                        installed.app, app.private_key.secret
                    )),
                    Some(app) if reading.is_some_and(|reading| reading.issuer == app.issuer) => {
                        Some(format!(
                            "the App {} is the App the source reads as, under another table; re-runs are made as another App, installed with actions: write alone",
                            installed.app
                        ))
                    }
                    Some(app) if origin(&app.api) != origin(&installed.api) => Some(format!(
                        "the App mints its tokens at {}, and the source reads {}; a write credential is sent to the source's own API alone",
                        app.api, installed.api
                    )),
                    Some(_) => None,
                },
            };
            if let Some(refusal) = refusal {
                self.error(&span, &path, refusal);
            }
        }
    }
}

/// `config`, once `loader` found each write credential of its sources
/// held apart from everything else it names.
///
/// # Errors
///
/// Where `loader` found an error.
pub(super) fn apart(
    config: Config,
    mut loader: Loader<'_>,
    spans: &BTreeMap<SourceName, Range<usize>>,
) -> Result<Config, Errors> {
    loader.kept_apart(&config, spans);
    if loader.errors.is_empty() {
        Ok(config)
    } else {
        Err(loader.failed())
    }
}

impl Loader<'_> {
    /// Each write credential of `config`'s sources refused where a run of
    /// any use reads its secret for anything else (a source, a destination,
    /// an App a source reads as, a webhook, the static log's token, the
    /// store), another write names it at another API or as another kind of
    /// credential, or a source reads as its App, under any table.
    pub(super) fn kept_apart(
        &mut self,
        config: &Config,
        spans: &BTreeMap<SourceName, Range<usize>>,
    ) {
        let named = config.secrets_read();
        let read_as: Vec<(&str, &GithubApp)> = config
            .sources
            .values()
            .filter_map(|source| source.installed())
            .filter_map(|installed| config.github_apps.get_key_value(installed.app))
            .map(|(name, app)| (name.as_str(), app))
            .collect();
        let mut written: BTreeMap<&str, (&SourceName, &str, Option<String>)> = BTreeMap::new();
        for (name, source) in &config.sources {
            let Some(write) = source.write() else {
                continue;
            };
            let Some((secret, kind, at)) = write_secret(&write, &config.github_apps) else {
                continue;
            };
            let read = match &write {
                WriteCredential::App(installed) => {
                    let app = config.github_apps.get(installed.app);
                    read_as.iter().any(|(reading, read)| {
                        *reading == installed.app
                            || app.is_some_and(|app| app.issuer == read.issuer)
                    })
                }
                WriteCredential::Token { .. } => false,
            };
            let refusal = if read {
                Some(format!(
                    "a source reads as the App {}, under this table or another; re-runs are made as an App no source reads as, installed with actions: write alone",
                    write.name()
                ))
            } else if named.contains(secret) {
                Some(format!(
                    "the write {kind} is held in the secret {secret}, which herald also reads for a source, a destination, an App, a webhook, the static log or the store; give the write credential a secret no other key names"
                ))
            } else {
                match written.insert(secret, (name, kind, at.clone())) {
                    Some((other, other_kind, other_at)) if other_kind != kind || other_at != at => {
                        Some(format!(
                            "the write {kind} is held in the secret {secret}, which the source {} writes with at another API or as another credential; a write credential is sent to one API alone",
                            other.as_str()
                        ))
                    }
                    _ => None,
                }
            };
            if let Some(refusal) = refusal {
                let span = spans.get(name).cloned().unwrap_or_default();
                self.error(&span, &format!("sources.{}.write", name.as_str()), refusal);
            }
        }
    }
}

#[cfg(test)]
mod tests;

//! The write credential each source re-runs with and each merge queue
//! takes its steps with, refused where it is the credential its source
//! reads with, by name or through the same GitHub App's key, where herald
//! reads its secret for anything else, or where it is sent anywhere but
//! the source's own API; and an operator's sign-in, refused where it is a
//! write App's own (ADR-0068, ADR-0087).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{Address, SourceName};
use herald_kinds::{Source, WriteCredential};

use super::Loader;
use crate::Config;
use crate::config::GithubApp;
use crate::error::Errors;
use crate::merging::MergeQueue;

/// Where each source's table and each merge queue's is written.
pub(super) type Spans<'s> = (
    &'s BTreeMap<SourceName, Range<usize>>,
    &'s BTreeMap<String, Range<usize>>,
);

/// What a write credential makes: a source's re-runs, or a merge queue's
/// steps.
#[derive(Debug, Clone, Copy)]
enum Writes {
    /// A source's re-runs (ADR-0068).
    Reruns,
    /// A merge queue's steps (ADR-0087).
    Merges,
}

impl Writes {
    /// What is made with it, as a refusal says.
    const fn made(self) -> &'static str {
        match self {
            Self::Reruns => "re-runs are made",
            Self::Merges => "the queue's steps are made",
        }
    }

    /// What its App is best installed with, as a refusal says.
    const fn alone(self) -> &'static str {
        match self {
            Self::Reruns => "installed with actions: write alone",
            Self::Merges => "installed with the permissions its steps need alone",
        }
    }
}

/// A write credential the config names: where, for which source, and what
/// it makes.
struct Writer<'a> {
    /// The key it is written at.
    path: String,
    /// Where its table is written.
    span: Range<usize>,
    /// The source it writes to.
    source: &'a Source,
    /// The credential.
    write: WriteCredential<'a>,
    /// What it makes.
    writes: Writes,
}

/// Each write credential of `sources` and of `queues`, the queues' each
/// with the source it names.
fn writers<'a>(
    sources: &'a BTreeMap<SourceName, Source>,
    queues: &'a BTreeMap<String, MergeQueue>,
    (source_spans, queue_spans): Spans<'_>,
) -> Vec<Writer<'a>> {
    let reruns = sources.iter().filter_map(|(name, source)| {
        Some(Writer {
            path: format!("sources.{}.write", name.as_str()),
            span: source_spans.get(name).cloned().unwrap_or_default(),
            source,
            write: source.write()?,
            writes: Writes::Reruns,
        })
    });
    let merges = queues.iter().filter_map(|(name, queue)| {
        Some(Writer {
            path: format!("merge_queues.{name}.write"),
            span: queue_spans.get(name).cloned().unwrap_or_default(),
            source: sources.get(&queue.source)?,
            write: queue.writing()?,
            writes: Writes::Merges,
        })
    });
    reruns.chain(merges).collect()
}

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
    /// Each write credential of `sources` and of `queues`, refused where
    /// it is one its source reads with, or an App of `apps` that is
    /// missing, is the App its source reads as under another table, or
    /// mints at another origin than its source reads; and each source's
    /// sign-in, refused where it is a write App's.
    pub(super) fn written(
        &mut self,
        (sources, queues): (&BTreeMap<SourceName, Source>, &BTreeMap<String, MergeQueue>),
        spans: Spans<'_>,
        apps: &BTreeMap<String, GithubApp>,
    ) {
        let writers = writers(sources, queues, spans);
        let issuers: BTreeSet<&str> = writers
            .iter()
            .filter_map(|writer| match &writer.write {
                WriteCredential::App(installed) => apps.get(installed.app),
                WriteCredential::Token { .. } => None,
            })
            .map(|app| app.issuer.as_str())
            .collect();
        for (name, source) in sources {
            if let Ok(signing) = source.signing()
                && issuers.contains(signing.client_id.as_str())
            {
                let span = spans.0.get(name).cloned().unwrap_or_default();
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
        for writer in &writers {
            let read = read_secrets(writer.source, apps);
            let reading = writer
                .source
                .installed()
                .and_then(|installed| apps.get(installed.app));
            let made = writer.writes.made();
            let refusal = match &writer.write {
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
                        "the App {} signs with the secret {}, which the source reads with; {made} as another App, with a key of its own",
                        installed.app, app.private_key.secret
                    )),
                    Some(app) if reading.is_some_and(|reading| reading.issuer == app.issuer) => {
                        Some(format!(
                            "the App {} is the App the source reads as, under another table; {made} as another App, {}",
                            installed.app,
                            writer.writes.alone()
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
                self.error(&writer.span, &writer.path, refusal);
            }
        }
    }
}

/// `config`, once `loader` found each write credential of its sources and
/// merge queues held apart from everything else it names.
///
/// # Errors
///
/// Where `loader` found an error.
pub(super) fn apart(
    config: Config,
    mut loader: Loader<'_>,
    spans: Spans<'_>,
) -> Result<Config, Errors> {
    loader.kept_apart(&config, spans);
    if loader.errors.is_empty() {
        Ok(config)
    } else {
        Err(loader.failed())
    }
}

impl Loader<'_> {
    /// Each write credential of `config`'s sources and merge queues
    /// refused where a run of any use reads its secret for anything else
    /// (a source, a destination, an App a source reads as, a webhook, the
    /// static log's token, the store), another write names it at another
    /// API or as another kind of credential, or a source reads as its App,
    /// under any table.
    pub(super) fn kept_apart(&mut self, config: &Config, spans: Spans<'_>) {
        let named = config.secrets_read();
        let read_as: Vec<(&str, &GithubApp)> = config
            .sources
            .values()
            .filter_map(|source| source.installed())
            .filter_map(|installed| config.github_apps.get_key_value(installed.app))
            .map(|(name, app)| (name.as_str(), app))
            .collect();
        let mut written: BTreeMap<&str, (String, &str, Option<String>)> = BTreeMap::new();
        for writer in writers(&config.sources, &config.merge_queues, spans) {
            let write = &writer.write;
            let Some((secret, kind, at)) = write_secret(write, &config.github_apps) else {
                continue;
            };
            let read = match write {
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
                    "a source reads as the App {}, under this table or another; {} as an App no source reads as, {}",
                    write.name(),
                    writer.writes.made(),
                    writer.writes.alone()
                ))
            } else if named.contains(secret) {
                Some(format!(
                    "the write {kind} is held in the secret {secret}, which herald also reads for a source, a destination, an App, a webhook, the static log or the store; give the write credential a secret no other key names"
                ))
            } else {
                match written.insert(secret, (writer.path.clone(), kind, at.clone())) {
                    Some((other, other_kind, other_at)) if other_kind != kind || other_at != at => {
                        Some(format!(
                            "the write {kind} is held in the secret {secret}, which {other} names at another API or as another credential; a write credential is sent to one API alone"
                        ))
                    }
                    _ => None,
                }
            };
            if let Some(refusal) = refusal {
                self.error(&writer.span, &writer.path, refusal);
            }
        }
    }
}

#[cfg(test)]
mod tests;

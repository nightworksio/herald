//! The GitHub Apps sources read as (ADR-0041): read after the sources, each
//! source naming one checked against it.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{Address, SourceName, kind_name, web_url};
use herald_kinds::{ConfiguredStore, Source};

use super::{Loader, Value};
use crate::config::GithubApp;
use crate::file::GithubAppFile;
use crate::merging::MergeQueue;

/// Whether `id` could be a GitHub App's client id: letters, digits, `.`,
/// `_` and `-`.
fn client_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl Loader<'_> {
    /// The GitHub Apps `value` gives, each checked against the sources of
    /// `sourced` that read or write as one and the merge queues of `queued`
    /// that write as one.
    pub(super) fn apps_of(
        &mut self,
        value: Option<Value<'_>>,
        store: Option<&ConfiguredStore>,
        sourced: (
            &BTreeMap<SourceName, Source>,
            &BTreeMap<SourceName, Range<usize>>,
        ),
        queued: (
            &BTreeMap<String, MergeQueue>,
            &BTreeMap<String, Range<usize>>,
        ),
    ) -> BTreeMap<String, GithubApp> {
        let apps = self.github_apps(value, store);
        self.installed(sourced, &apps);
        self.written((sourced.0, queued.0), (sourced.1, queued.1), &apps);
        apps
    }

    /// Each App of `[github_apps]`.
    pub(super) fn github_apps(
        &mut self,
        value: Option<Value<'_>>,
        store: Option<&ConfiguredStore>,
    ) -> BTreeMap<String, GithubApp> {
        let mut apps = BTreeMap::new();
        for (name, value) in self.named(value, "github_apps") {
            let path = format!("github_apps.{name}");
            let span = value.span();
            let Some(app) = self.typed::<GithubAppFile>(value, &path) else {
                continue;
            };
            if !kind_name(&name) {
                self.error(
                    &span,
                    &path,
                    format!("\"{name}\" names its webhook's path; give a name of lowercase letters, digits and inner '-'"),
                );
                continue;
            }
            let issuer = match (app.app_id, app.client_id) {
                (Some(id), None) => id.to_string(),
                (None, Some(id)) if client_id(&id) => id,
                (None, Some(id)) => {
                    self.error(
                        &span,
                        &format!("{path}.client_id"),
                        format!("\"{id}\" is not a client id; give it as the App's settings page shows it, such as \"Iv23liABCDEF\""),
                    );
                    continue;
                }
                (Some(_), Some(_)) | (None, None) => {
                    self.error(
                        &span,
                        &path,
                        "an App's tokens are issued by its client id or its id; give client_id or app_id, one of them",
                    );
                    continue;
                }
            };
            let api = app.api.unwrap_or_else(|| GithubApp::API.to_owned());
            if !web_url(&api) {
                self.error(
                    &span,
                    &format!("{path}.api"),
                    format!(
                        "api \"{api}\" is not a web address; give the REST API, such as {}",
                        GithubApp::API
                    ),
                );
                continue;
            }
            let store_token = app.store_token.unwrap_or_default();
            if store_token && let Some(store) = store.filter(|store| !store.keeps_tokens()) {
                self.error(
                    &span,
                    &format!("{path}.store_token"),
                    format!("the {} store keeps no records, so it keeps no token; choose a store that keeps records, or remove store_token", store.kind()),
                );
                continue;
            }
            apps.insert(
                name,
                GithubApp {
                    issuer,
                    private_key: app.private_key,
                    api,
                    webhook: app.webhook,
                    store_token,
                },
            );
        }
        apps
    }

    /// Each source of `sources` that reads as a GitHub App, refused where
    /// `apps` has no App of its name or the App mints at another host than
    /// the one the source reads.
    pub(super) fn installed(
        &mut self,
        (sources, spans): (
            &BTreeMap<SourceName, Source>,
            &BTreeMap<SourceName, Range<usize>>,
        ),
        apps: &BTreeMap<String, GithubApp>,
    ) {
        let origin = |url: &str| Address::parse(url).map(|address| address.origin());
        for (name, source) in sources {
            let Some(installed) = source.installed() else {
                continue;
            };
            let span = spans.get(name).cloned().unwrap_or_default();
            let path = format!("sources.{}.app", name.as_str());
            match apps.get(installed.app) {
                None => self.error(
                    &span,
                    &path,
                    format!(
                        "no [github_apps.{}] is given; give it, or name one of {}",
                        installed.app,
                        super::list(&apps.keys().map(String::as_str).collect::<Vec<_>>())
                    ),
                ),
                Some(app) if origin(&app.api) != origin(&installed.api) => self.error(
                    &span,
                    &path,
                    format!(
                        "the App mints its tokens at {}, and the source reads {}; give the App the source's api, or the source the App's",
                        app.api, installed.api
                    ),
                ),
                Some(_) => {}
            }
        }
    }
}

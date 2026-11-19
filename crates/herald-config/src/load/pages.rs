//! `[federation.pages]`: where herald writes its feeds as a static signed
//! log, served on `[federation]`'s domain, signed by its keys, and kept in
//! a store that keeps records (ADR-0060).

use std::ops::Range;

use herald_kinds::{ConfiguredStore, StoreKind};
use herald_peers::discovery::address;
use herald_store_git_branch::GitBranchKind;

use super::Loader;
use crate::config::Pages;
use crate::file::PagesFile;

/// Where errors in `[federation.pages]` are reported.
const AT: &str = "federation.pages";

impl Loader<'_> {
    /// `[federation.pages]` as `file` writes it, where it is sound: served
    /// on `domain`, signed by `[federation]`'s keys where it has any, `keyed`,
    /// and kept in `store`.
    pub(super) fn pages(
        &mut self,
        span: &Range<usize>,
        file: &PagesFile,
        (domain, keyed): (Option<&str>, bool),
        store: Option<&ConfiguredStore>,
    ) -> Option<Pages> {
        let Some((url, host)) = address(&file.url) else {
            self.error(
                span,
                &format!("{AT}.url"),
                format!(
                    "\"{}\" is not where a static host serves the log; give https://, the domain and a path, such as \"https://owner.github.io/repository\"",
                    file.url
                ),
            );
            return None;
        };
        if domain != Some(host.as_str()) {
            self.error(
                span,
                &format!("{AT}.url"),
                format!(
                    "the log is served on {host}, but [federation] domain is {}; serve it on the domain its keys are published on",
                    domain.unwrap_or("not given")
                ),
            );
        }
        if !keyed {
            self.error(
                span,
                AT,
                "the static log is signed by [federation]'s newest key; give keys in [federation]",
            );
        }
        if let Some(store) = store.filter(|store| store.reads_posts()) {
            self.error(
                span,
                AT,
                format!(
                    "the {} store keeps what was announced in the posts it reads back, and the static log's sequence is kept in none; give a store that keeps records, such as \"{}\"",
                    store.kind(),
                    GitBranchKind::NAME
                ),
            );
        }
        if let Some(branch) = file.branch.as_deref().filter(|branch| !branched(branch)) {
            self.error(
                span,
                &format!("{AT}.branch"),
                format!(
                    "\"{branch}\" is not a branch's name; give letters, digits, '-', '_', '.' and '/', such as \"{}\"",
                    Pages::BRANCH
                ),
            );
        }
        let refresh = self.lasting(
            (span.clone(), "pages.refresh".to_owned()),
            file.refresh,
            (Pages::REFRESH, Pages::REFRESH_ALLOWED, "7d"),
        );
        let base = url
            .strip_prefix(&format!("https://{host}"))
            .unwrap_or_default()
            .to_owned();
        let branch = match (&file.branch, &file.artifact) {
            (None, Some(_)) => None,
            (branch, _) => Some(branch.clone().unwrap_or_else(|| Pages::BRANCH.to_owned())),
        };
        Some(Pages {
            url,
            base,
            branch,
            repository: file.repository.clone(),
            token: file.token.clone(),
            api: file.api.clone(),
            artifact: file.artifact.clone(),
            refresh,
        })
    }
}

/// Whether `branch` names a branch: letters, digits, `-`, `_`, `.` and
/// `/`, starting with neither `.`, `/` nor `-`, ending with neither `.`
/// nor `/`, and holding no `..` or `//`.
fn branched(branch: &str) -> bool {
    !branch.is_empty()
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        && !branch.starts_with(['.', '/', '-'])
        && !branch.ends_with(['.', '/'])
        && !branch.contains("..")
        && !branch.contains("//")
}

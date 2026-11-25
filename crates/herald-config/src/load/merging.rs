//! The merge queues (ADR-0087): each naming a forge's source, a repository
//! of it and a branch no other table names, refused where a key cannot
//! hold or holds nothing for who runs it.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::merging::{
    Enter, Message, Runs, SKEW, UPDATE_WAIT, UPDATE_WAIT_MOST, WAIT, Write,
};
use herald_core::{Duration, SourceName};
use herald_kinds::{MergeAsked, Source};

use super::{Loader, Value, list};
use crate::merging::{MergeQueue, MergeQueueFile};

/// Why a blank branch is refused.
const BRANCHLESS: &str = "give the base branch the queue merges into, such as branch = \"main\"";

/// Why a blank gesture is refused.
const UNENTERED: &str = "a pull request is queued by a label, or a comment whose whole body is a command, that is not blank; such as enter = { label = \"herald:merge\" }";

/// Why a check named blank is refused.
const UNNAMED_CHECK: &str = "a check is named as the forge shows it, not blank";

/// Why `retries` naming no check is refused.
const UNRETRIED: &str =
    "retries names the checks it re-runs, such as retries = { checks = [\"test\"], times = 2 }";

/// Why `message = "pull"` beside another method is refused.
const UNMESSAGED: &str = "message = \"pull\" sets the commit message of a merge or a squash alone; give method = \"merge\" or \"squash\", or remove message";

/// Why `role_lost` beside `runs = "herald"` is refused.
const UNFORGED_ROLE_LOST: &str = "role_lost says what herald does with a pull request it handed to the forge's queue, and with runs = \"herald\" herald hands none over; give runs = \"forge\", or remove role_lost";
/// Why `signer` is refused where the forge is not GitHub, or it is blank.
const UNSIGNING: &str = "signer names the committer a GitHub Enterprise Server or GHE.com instance signs the commits it makes for, such as signer = \"noreply@ghe.example\"; GitLab and Forgejo give no such signature, so remove signer there";

/// The kind of a GitHub source, as a merge queue's forge names it.
const GITHUB: &str = "github";

/// The keys that shape herald's own queue alone, unread where the forge
/// runs it.
const HERALD_ONLY: [&str; 6] = [
    "may_fail",
    "wait",
    "retries",
    "updated",
    "signer",
    "update_wait",
];

/// The names of `sources` a merge queue may name.
fn forges(sources: &BTreeMap<SourceName, Source>) -> Vec<&str> {
    let asked = MergeAsked {
        repository: None,
        method: None,
        retries: false,
        write: None,
    };
    sources
        .iter()
        .filter(|(_, source)| source.merging(&asked).is_some())
        .map(|(name, _)| name.as_str())
        .collect()
}

/// Whether `enter` names a label or a command that is blank.
fn blank(enter: &Enter) -> bool {
    match enter {
        Enter::Label(text) | Enter::Comment(text) => text.trim().is_empty(),
    }
}

/// The keys of herald's own queue `file` gives.
fn herald_keys(file: &MergeQueueFile) -> Vec<&'static str> {
    let given = [
        !file.may_fail.is_empty(),
        file.wait.is_some(),
        file.retries.is_some(),
        file.updated.is_some(),
        file.signer.is_some(),
        file.update_wait.is_some(),
    ];
    HERALD_ONLY
        .into_iter()
        .zip(given)
        .filter_map(|(key, given)| given.then_some(key))
        .collect()
}

impl Loader<'_> {
    /// The merge queues `value` gives, each on a source of `sources`, with
    /// where each is written.
    pub(super) fn merge_queues(
        &mut self,
        value: Option<Value<'_>>,
        sources: &BTreeMap<SourceName, Source>,
    ) -> (BTreeMap<String, MergeQueue>, BTreeMap<String, Range<usize>>) {
        let mut queues = BTreeMap::new();
        let mut spans = BTreeMap::new();
        let mut branches: BTreeMap<(String, String, String), String> = BTreeMap::new();
        for (name, value) in self.named(value, "merge_queues") {
            let path = format!("merge_queues.{name}");
            let span = value.span();
            let Some(file) = self.typed::<MergeQueueFile>(value, &path) else {
                continue;
            };
            let Some(queue) = self.merge_queue(file, (&path, &span), sources) else {
                continue;
            };
            let merged = (
                queue.forge.api.clone(),
                queue.forge.repository.clone(),
                queue.branch.clone(),
            );
            if let Some(other) = branches.insert(merged, name.clone()) {
                self.error(
                    &span,
                    &format!("{path}.branch"),
                    format!(
                        "the queue {other} merges into {} of {} already; give one table per branch",
                        queue.branch, queue.forge.repository
                    ),
                );
                continue;
            }
            spans.insert(name.clone(), span);
            queues.insert(name, queue);
        }
        (queues, spans)
    }

    /// Each key of the merge queue `file` at `path` its `runs` leaves
    /// unread or refuses.
    fn ran(&mut self, file: &MergeQueueFile, runs: Runs, (path, span): (&str, &Range<usize>)) {
        let at = |key: &str| format!("{path}.{key}");
        let unread = if runs == Runs::Forge {
            herald_keys(file)
        } else {
            Vec::new()
        };
        for key in unread {
            self.error(
                span,
                &at(key),
                format!("{key} shapes herald's own queue, and with runs = \"forge\" the forge proves each pull request; give runs = \"herald\", or remove {key}"),
            );
        }
        if runs == Runs::Herald && file.role_lost.is_some() {
            self.error(span, &at("role_lost"), UNFORGED_ROLE_LOST);
        }
    }

    /// The merge queue `file` at `path`, where each of its keys holds.
    fn merge_queue(
        &mut self,
        file: MergeQueueFile,
        (path, span): (&str, &Range<usize>),
        sources: &BTreeMap<SourceName, Source>,
    ) -> Option<MergeQueue> {
        let errors = self.errors.len();
        let at = |key: &str| format!("{path}.{key}");
        let runs = file.runs.unwrap_or_default();
        self.ran(&file, runs, (path, span));
        let enter = file.enter.unwrap_or_default();
        if file.branch.trim().is_empty() {
            self.error(span, &at("branch"), BRANCHLESS);
        }
        if blank(&enter) {
            self.error(span, &at("enter"), UNENTERED);
        }
        let retried = file
            .retries
            .as_ref()
            .map_or(&[][..], |retries| &retries.checks);
        for (key, checks) in [
            ("may_fail", &file.may_fail[..]),
            ("retries.checks", retried),
        ] {
            if checks.iter().any(|check| check.trim().is_empty()) {
                self.error(span, &at(key), UNNAMED_CHECK);
            }
        }
        if file.retries.is_some() && retried.is_empty() {
            self.error(span, &at("retries.checks"), UNRETRIED);
        }
        if file.message == Some(Message::Pull)
            && file.method.is_some_and(|method| !method.messaged())
        {
            self.error(span, &at("message"), UNMESSAGED);
        }
        if let Some(Err(why)) = file.write.as_ref().map(Write::check) {
            self.error(span, &at("write"), why);
        }
        let name = SourceName::new(file.source.get_ref());
        let asked = MergeAsked {
            repository: file.repository.as_deref(),
            method: file.method,
            retries: file.retries.is_some(),
            write: file.write.as_ref(),
        };
        let forge = match sources.get(&name).map(|source| source.merging(&asked)) {
            Some(Some(Ok(forge))) => forge,
            Some(Some(Err(unmerged))) => {
                self.error(span, &at(unmerged.key), unmerged.why);
                return None;
            }
            given => {
                let why = if given.is_some() {
                    "is no source of kind \"github\", \"gitlab\" or \"forgejo\""
                } else {
                    "is no source"
                };
                self.error(
                    &file.source.span(),
                    &at("source"),
                    format!(
                        "\"{}\" {why}; the sources a merge queue may name are {}",
                        name.as_str(),
                        list(&forges(sources))
                    ),
                );
                return None;
            }
        };
        let unsigning = |signer: &str| forge.forge != GITHUB || signer.trim().is_empty();
        if file.signer.as_deref().is_some_and(unsigning) {
            self.error(span, &at("signer"), UNSIGNING);
        }
        let update_wait = file.update_wait.map_or(UPDATE_WAIT, |wait| wait.0);
        if update_wait > UPDATE_WAIT_MOST {
            let most = Duration(UPDATE_WAIT_MOST).written();
            self.error(
                span,
                &at("update_wait"),
                format!("update_wait is at most \"{most}\"; shorten it"),
            );
        }
        (self.errors.len() == errors).then(|| MergeQueue {
            source: name,
            forge,
            branch: file.branch,
            runs,
            enter,
            least: file.least.unwrap_or_default(),
            vouches: file.vouches.unwrap_or_default(),
            skew: file.skew.map_or(SKEW, |skew| skew.0),
            updated: file.updated.unwrap_or_default(),
            signer: file.signer,
            update_wait,
            method: file.method,
            message: file.message.unwrap_or_default(),
            may_fail: file.may_fail,
            wait: file.wait.map_or(WAIT, |wait| wait.0),
            retries: file.retries,
            applied_by: file.applied_by.unwrap_or_default(),
            role_lost: file.role_lost.unwrap_or_default(),
            write: file.write,
        })
    }
}

#[cfg(test)]
mod tests;

use std::fmt;

use herald_core::{Listed, Owner, Pull, Rerun, Validators, Visibility};
use herald_ports::{Boxed, DynHttp};

use crate::Unpolled;

/// The most open pull requests a repository's listing reads by default.
pub const MOST_OPEN: usize = 500;

/// The most open pull requests of a repository read in full by default,
/// where each takes requests of its own.
pub const MOST_PULLS: usize = 100;

/// How many open pull requests of a repository a view reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Most {
    /// The most it lists.
    pub open: usize,
    /// The most it reads in full, where each takes requests of its own;
    /// those past it read as unknown.
    pub pulls: usize,
}

impl Default for Most {
    fn default() -> Self {
        Self {
            open: MOST_OPEN,
            pulls: MOST_PULLS,
        }
    }
}

/// The repositories of an owner source a view reads (ADR-0062).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope<'a> {
    /// What kind of account the owner is, as its last listing found.
    pub owner: Owner,
    /// Each repository its selection takes, as its membership record holds
    /// it: what names each pull request's project and who may see it.
    pub members: &'a [Listed],
}

/// How a view asks a source for its open pull requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Asked<'a> {
    /// How many it reads.
    pub most: Most,
    /// The owner's repositories it reads, where the source names an owner
    /// rather than one repository.
    pub scope: Option<Scope<'a>>,
    /// Whether a live read through a REST listing without a token takes
    /// who may see the repository from what the listing states, rather than
    /// taking each pull request as private (ADR-0062).
    pub stated: bool,
    /// Whether each pull request read in full has the paths its head
    /// changes read too, for a view predicting conflicts (ADR-0063).
    pub files: bool,
}

/// What reading a source's open pull requests costs each round, as doctor
/// shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueCost {
    /// How they are read, such as `graphql`, `listing` or `rest`.
    pub path: &'static str,
    /// The most requests a round takes.
    pub requests: usize,
    /// Whether each pull request takes requests of its own, those past
    /// [`Most::pulls`] reading as unknown.
    pub detailed: bool,
    /// Whether it reads whether each can be merged; a batch of bumps counts
    /// none green where it does not.
    pub mergeability: bool,
    /// Whether it reads an owner's repositories by one query over them,
    /// each repository listed on its own past what that query lists
    /// (ADR-0062).
    pub scoped: bool,
    /// The most requests reading the paths each pull request's head
    /// changes adds a round, where a view predicts conflicts (ADR-0063).
    pub files: usize,
}

/// What a reconcile's listing of a repository's open pull requests found
/// (ADR-0061).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relisted {
    /// Nothing changed since the validators it was asked under.
    Unchanged,
    /// The open pull requests, each read as far as the listing reads it.
    Listed {
        /// The pull requests.
        pulls: Vec<Pull>,
        /// The forge's id of their repository, where the listing names one.
        repository: Option<String>,
        /// Who may see their repository, where the listing names it: what
        /// narrows kept records alone, never widening one (ADR-0061).
        visibility: Option<Visibility>,
        /// The validators each page was answered with, in order.
        validators: Vec<Validators>,
    },
}

/// What a rule's plan to re-run a check found (ADR-0065).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Each failed run on the head that ran the check, with the exact call
    /// re-running its failed jobs.
    Runs(Vec<Rerun>),
    /// Why no run can be re-run, in plain words.
    Unplanned(&'static str),
}

impl Relisted {
    /// The pull requests it listed, none where nothing changed.
    #[must_use]
    pub fn pulls(self) -> Vec<Pull> {
        match self {
            Self::Unchanged => Vec::new(),
            Self::Listed { pulls, .. } => pulls,
        }
    }
}

/// A source's repository's open pull requests, read for a view.
pub trait Queue: fmt::Debug + Send + Sync {
    /// Whether it reads an owner's repositories, so a view asks it with
    /// their [`Scope`].
    fn scoped(&self) -> bool;

    /// Its open pull requests, as `asked`, asked through `http`.
    fn opened<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
    ) -> Boxed<'a, Result<Vec<Pull>, Unpolled>>;

    /// Its open pull requests, at most `asked.most.open`, each read as far
    /// as a listing reads it, asked through `http` conditional on
    /// `validators`, the last answer of each page (ADR-0061).
    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
        validators: &'a [Validators],
    ) -> Boxed<'a, Result<Relisted, Unpolled>>;

    /// Whether it reads the repository at the full path `repository`.
    fn holds(&self, repository: &str) -> bool;

    /// The failed runs on a head of a repository that ran a check, `ran`
    /// being the repository's full path, the head's sha and the check's
    /// name, the newest `most` read, each with the exact call re-running
    /// its failed jobs, asked through `http`; or why none can be re-run
    /// (ADR-0065).
    fn planned<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        ran: (&'a str, &'a str, &'a str),
        most: usize,
    ) -> Boxed<'a, Result<Plan, Unpolled>>;

    /// `pulls`, as [`Queue::listed`] found them, each read in full.
    fn completed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
        pulls: Vec<Pull>,
    ) -> Boxed<'a, Result<Vec<Pull>, Unpolled>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relisting_gives_the_pull_requests_it_listed_and_none_where_nothing_changed() {
        assert_eq!(Relisted::Unchanged.pulls(), Vec::<Pull>::new());
        let listed = Relisted::Listed {
            pulls: Vec::new(),
            repository: Some("1".to_owned()),
            visibility: None,
            validators: Vec::new(),
        };
        assert_eq!(listed.pulls(), Vec::<Pull>::new());
        let pull: Pull = serde_json::from_value(serde_json::json!({
            "project": "r", "repository": "o/r", "visibility": "private", "number": 1,
            "title": "t", "url": "https://github.com/o/r/pull/1", "author": null,
            "bot": false, "draft": false, "labels": [],
            "opened": "2026-11-18T10:00:00Z", "updated": "2026-11-18T11:00:00Z",
            "review": "unasked", "reviewers": [], "checks": "absent", "mergeable": "unknown",
            "queued": false, "forge": null, "head": null, "unread": false
        }))
        .unwrap();
        let listed = Relisted::Listed {
            pulls: vec![pull.clone()],
            repository: None,
            visibility: None,
            validators: Vec::new(),
        };
        assert_eq!(listed.pulls(), [pull]);
        assert!(!Asked::default().stated);
        assert!(!Asked::default().files);
        assert_eq!(Asked::default().most, Most::default());
    }
}

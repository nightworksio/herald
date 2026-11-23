//! A view's overviews beside its rows: its repositories' CI capacity, the
//! runs queued and running and those superseded by a newer push
//! (ADR-0066), and their advisories as its audience may see them
//! (ADR-0067).

use jiff::Timestamp;

use super::advisory::Advised;
use super::header::Header;
use super::load::Loads;
use super::{Pull, View};
use crate::members::Visibility;

/// The most repositories a view reads capacity of where it reads its
/// owner's members, the newest pushed first, where it names no number.
pub const CAPACITY_REPOSITORIES: usize = 20;

/// The most repositories a view may read capacity of.
pub const MOST_CAPACITY_REPOSITORIES: usize = 100;

/// Which repositories a view reads capacity of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capacity {
    /// Those it shows a pull request of.
    Shown,
    /// Its owner's members, the newest pushed first, at most `most`.
    Members {
        /// The most read.
        most: usize,
    },
}

/// One CI run, queued or running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The repository's full path.
    pub repository: String,
    /// Who may see the repository.
    pub visibility: Visibility,
    /// Its id on the forge.
    pub id: String,
    /// Its page.
    pub url: String,
    /// The commit it runs on.
    pub sha: String,
    /// Whether it was started for a pull request.
    pub pull: bool,
    /// Whether it is still queued.
    pub queued: bool,
    /// When it was created.
    pub created: Timestamp,
}

/// What of an overview could not be read, and why, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    /// The repository's full path.
    pub repository: String,
    /// Who may see the repository.
    pub visibility: Visibility,
    /// Why it could not be read.
    pub reason: String,
}

impl Unread {
    /// Whether the view `view` names it: a repository `sourced` names only
    /// where the view holds it, an owner or a source always, so a read that
    /// failed is never shown as clean and no hidden repository is named.
    /// Every overview's unread entries are narrowed by it alone.
    #[must_use]
    pub fn within(&self, view: &View, sourced: impl Fn(&str) -> bool) -> bool {
        !sourced(&self.repository) || view.holds(&self.repository)
    }
}

/// The commit an open pull request's head is at, in its repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The repository the pull request is opened in.
    pub repository: String,
    /// The commit.
    pub sha: String,
}

/// What a view's overviews found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overviews {
    /// The queued and running runs, where the view shows capacity.
    pub capacity: Option<Vec<Running>>,
    /// What could not be read.
    pub unread: Vec<Unread>,
    /// The head of every open pull request the view read, in its
    /// repository.
    pub heads: Vec<Head>,
    /// The advisories section, as the surface it leaves through is shown
    /// it, where the view shows one.
    pub advisories: Option<Advised>,
    /// The load per team and per area, counted from the rows one surface
    /// shows, where the view shows it (ADR-0075).
    pub load: Option<Loads>,
    /// The release's header, where the view reads one (ADR-0082).
    pub header: Option<Header>,
}

impl Overviews {
    /// It as the view `view` shows it: only the runs of the repositories
    /// the view holds, and of what could not be read, a repository
    /// `sourced` names only where the view holds it, an owner or a source
    /// always, so a read that failed is never shown as clean and nothing
    /// of a hidden repository leaves through it.
    #[must_use]
    pub fn within(mut self, view: &View, sourced: impl Fn(&str) -> bool) -> Self {
        if let Some(runs) = &mut self.capacity {
            runs.retain(|run| view.holds(&run.repository));
        }
        self.unread.retain(|unread| unread.within(view, &sourced));
        self.header = self.header.map(|header| header.within(view));
        self
    }

    /// The head of each of `pulls` that names one, in its repository.
    #[must_use]
    pub fn heads_of(pulls: &[Pull]) -> Vec<Head> {
        pulls
            .iter()
            .filter_map(|pull| {
                pull.head.as_ref().map(|sha| Head {
                    repository: pull.repository.clone(),
                    sha: sha.clone(),
                })
            })
            .collect()
    }

    /// It with `other`'s runs, unread repositories and header added.
    pub fn merge(&mut self, other: Self) {
        if let Some(runs) = other.capacity {
            self.capacity.get_or_insert_default().extend(runs);
        }
        self.unread.extend(other.unread);
        if let Some(header) = other.header {
            self.header.get_or_insert_default().merge(header);
        }
    }

    /// The runs started for a pull request on a commit no open pull
    /// request of its own repository has its head at: superseded by a
    /// newer push, or by its closing. A head in another repository never
    /// holds a run.
    #[must_use]
    pub fn superseded(&self) -> Vec<&Running> {
        self.capacity
            .iter()
            .flatten()
            .filter(|run| {
                run.pull
                    && !self.heads.iter().any(|head| {
                        head.sha == run.sha && head.repository.eq_ignore_ascii_case(&run.repository)
                    })
            })
            .collect()
    }

    /// It as an audience seeing no repository stricter than `visibility`
    /// may be shown it, with no load: a load is counted from the rows the
    /// audience is shown.
    #[must_use]
    pub fn seen_by(&self, visibility: Visibility) -> Self {
        Self {
            capacity: self.capacity.as_ref().map(|runs| {
                runs.iter()
                    .filter(|run| run.visibility <= visibility)
                    .cloned()
                    .collect()
            }),
            unread: self
                .unread
                .iter()
                .filter(|unread| unread.visibility <= visibility)
                .cloned()
                .collect(),
            heads: self.heads.clone(),
            advisories: self.advisories.clone(),
            load: None,
            header: self
                .header
                .as_ref()
                .map(|header| header.shown(|seen| seen <= visibility)),
        }
    }
}

#[cfg(test)]
mod tests;

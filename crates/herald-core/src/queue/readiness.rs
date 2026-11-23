//! A view's release readiness: the open pull requests its selectors pick,
//! and whether any of them holds the release back (ADR-0081).

use super::{Checks, Pull, Review, Row};
use crate::glob::Glob;

/// The most blocker labels a view names.
pub const MOST_BLOCKER_LABELS: usize = 50;

/// The most holds a verdict names; those past it are counted.
pub const MOST_HOLDS: usize = 100;

/// The most failed checks one hold names; those past it are counted.
pub const MOST_HELD_CHECKS: usize = 10;

/// The labels that block a release where a view names none.
pub const BLOCKER_LABELS: [&str; 2] = ["blocker", "release-blocker"];

/// The head branch prefix release-please opens its release pull request
/// on.
pub const RELEASE_PLEASE_HEAD: &str = "release-please--branches--";

/// The head branch prefix the changesets action opens its release pull
/// request on.
pub const CHANGESETS_HEAD: &str = "changeset-release/";

/// How a view's selectors combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Combine {
    /// A pull request is picked where every selector named picks it.
    #[default]
    All,
    /// A pull request is picked where any selector named picks it.
    Any,
}

/// What tells a release pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleasePull {
    /// release-please's: its head branch starts with
    /// [`RELEASE_PLEASE_HEAD`].
    ReleasePlease,
    /// The changesets action's: its head branch starts with
    /// [`CHANGESETS_HEAD`].
    Changesets,
    /// Its head branch matches the glob.
    Head(Glob),
    /// Its title matches the glob.
    Title(Glob),
}

impl ReleasePull {
    /// Whether `pull` is a release pull request by it.
    fn tells(&self, pull: &Pull) -> bool {
        let head = pull.head_branch.as_deref();
        match self {
            Self::ReleasePlease => {
                head.is_some_and(|branch| branch.starts_with(RELEASE_PLEASE_HEAD))
            }
            Self::Changesets => head.is_some_and(|branch| branch.starts_with(CHANGESETS_HEAD)),
            Self::Head(glob) => head.is_some_and(|branch| glob.matches(branch)),
            Self::Title(glob) => glob.matches(&pull.title),
        }
    }
}

/// What holds a release back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Blocker {
    /// A pull request carrying a blocker label.
    Label,
    /// A pull request with a failed check.
    Failed,
    /// A draft pull request.
    Draft,
    /// A pull request a reviewer asked to change.
    Changes,
}

impl Blocker {
    /// Every blocker, in the order a verdict names them.
    pub const ALL: [Self; 4] = [Self::Label, Self::Failed, Self::Draft, Self::Changes];

    /// The blocker as a config, a template and JSON name it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Label => "label",
            Self::Failed => "failed",
            Self::Draft => "draft",
            Self::Changes => "changes",
        }
    }
}

/// How a release is judged on a surface some of its pull requests are
/// withheld from, a private or internal repository's that the surface may
/// not see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Withheld {
    /// The verdict is not known, saying only that some pull requests are
    /// private.
    #[default]
    Unknown,
    /// The verdict is judged by the pull requests shown alone.
    Shown,
    /// The whole verdict, each withheld pull request holding the release
    /// back named private and nothing more.
    Masked,
}

/// A view's release: which open pull requests are its, and what holds it
/// back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The milestone a pull request carries, by title.
    pub milestone: Option<Glob>,
    /// A label a pull request carries.
    pub label: Option<Glob>,
    /// The branch a pull request merges into.
    pub branch: Option<Glob>,
    /// What tells a release pull request.
    pub pull: Option<ReleasePull>,
    /// How the selectors named combine.
    pub combine: Combine,
    /// The labels that block it.
    pub blocker_labels: Vec<Glob>,
    /// What holds it back.
    pub blocked_by: Vec<Blocker>,
    /// How it is judged where some of its pull requests are withheld.
    pub withheld: Withheld,
    /// Whether its header names the open milestones its milestone
    /// selector matches (ADR-0082).
    pub header: bool,
}

/// Why one pull request holds a release back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldBy {
    /// A blocker of the view's.
    Blocked(Blocker),
    /// It was listed past the most a view reads in full, so whether it
    /// blocks is not known.
    Unread,
}

impl HeldBy {
    /// The reason as a template and JSON name it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Blocked(blocker) => blocker.name(),
            Self::Unread => "unread",
        }
    }
}

/// One pull request holding a release back, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    /// Why.
    pub held_by: HeldBy,
    /// Its repository's full path.
    pub repository: String,
    /// Its number.
    pub number: u64,
    /// Its page.
    pub url: String,
    /// The blocker labels it carries, or the checks that failed, at most
    /// [`MOST_HELD_CHECKS`] of them.
    pub named: Vec<String>,
    /// How many more it carries or failed past those named.
    pub more: usize,
}

/// One hold on a release, as a surface is told it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// A pull request the surface is shown.
    Shown(Holding),
    /// A pull request withheld from it: nothing of it is told.
    Private,
}

/// What a verdict was judged on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Judged {
    /// Every pull request the release picks.
    #[default]
    Whole,
    /// Those shown alone, some being withheld.
    Shown,
    /// Nothing: some being withheld, the verdict is not known.
    Unknown,
}

/// Whether a release is ready, and what holds it back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Readiness {
    /// The holds, at most [`MOST_HOLDS`]: the shown ones in the rows'
    /// order, then the private ones.
    pub holds: Vec<Held>,
    /// How many holds past those named.
    pub more: usize,
    /// What it was judged on.
    pub judged: Judged,
}

impl Readiness {
    /// Whether nothing holds the release back, where that is known.
    #[must_use]
    pub fn ready(&self) -> Option<bool> {
        (self.judged != Judged::Unknown).then_some(self.holds.is_empty())
    }

    /// `hold` named, or counted past [`MOST_HOLDS`].
    fn held(&mut self, hold: Held) {
        if self.holds.len() == MOST_HOLDS {
            self.more += 1;
        } else {
            self.holds.push(hold);
        }
    }
}

/// `names`, at most [`MOST_HELD_CHECKS`] of them, and how many more.
fn capped(mut names: Vec<String>) -> (Vec<String>, usize) {
    let more = names.len().saturating_sub(MOST_HELD_CHECKS);
    names.truncate(MOST_HELD_CHECKS);
    (names, more)
}

impl Release {
    /// The milestone selector its header names the milestones of, where it
    /// shows a header.
    #[must_use]
    pub fn headed(&self) -> Option<&Glob> {
        self.milestone.as_ref().filter(|_| self.header)
    }

    /// Whether `pull` is the release's: each selector named picking it, or
    /// any one where they combine by any.
    #[must_use]
    pub fn picks(&self, pull: &Pull) -> bool {
        let named = |glob: &Glob, name: Option<&str>| name.is_some_and(|name| glob.matches(name));
        let picked = [
            self.milestone
                .as_ref()
                .map(|glob| named(glob, pull.milestone.as_deref())),
            self.label
                .as_ref()
                .map(|glob| pull.labels.iter().any(|label| glob.matches(label))),
            self.branch
                .as_ref()
                .map(|glob| named(glob, pull.base.as_deref())),
            self.pull.as_ref().map(|release| release.tells(pull)),
        ];
        let mut told = picked.into_iter().flatten().peekable();
        told.peek().is_some()
            && match self.combine {
                Combine::All => told.all(|picks| picks),
                Combine::Any => told.any(|picks| picks),
            }
    }

    /// What `blocker` names of `pull`, where it holds it back by it.
    fn blocked(&self, blocker: Blocker, pull: &Pull) -> Option<Vec<String>> {
        match blocker {
            Blocker::Label => {
                let carried: Vec<String> = pull
                    .labels
                    .iter()
                    .filter(|label| self.blocker_labels.iter().any(|glob| glob.matches(label)))
                    .cloned()
                    .collect();
                (!carried.is_empty()).then_some(carried)
            }
            Blocker::Failed => match &pull.checks {
                Checks::Failed(checks) => Some(checks.clone()),
                Checks::Absent | Checks::Pending | Checks::Passed => None,
            },
            Blocker::Draft => pull.draft.then(Vec::new),
            Blocker::Changes => (pull.review == Review::ChangesRequested).then(Vec::new),
        }
    }

    /// What holds `pull` back: each blocker the view names by it, or that
    /// it was not read in full.
    fn holds(&self, pull: &Pull) -> Vec<(HeldBy, Vec<String>)> {
        if pull.unread {
            return vec![(HeldBy::Unread, Vec::new())];
        }
        Blocker::ALL
            .into_iter()
            .filter(|blocker| self.blocked_by.contains(blocker))
            .filter_map(|blocker| {
                self.blocked(blocker, pull)
                    .map(|named| (HeldBy::Blocked(blocker), named))
            })
            .collect()
    }

    /// The verdict one surface is told: what holds each of `shown`, the
    /// rows it shows, back, and of `withheld`, the rows of the release
    /// withheld from it, only what [`Withheld`] lets it be told.
    #[must_use]
    pub fn verdict(&self, shown: &[&Row], withheld: &[&Row]) -> Readiness {
        let mut readiness = Readiness::default();
        for row in shown {
            for (held_by, named) in self.holds(&row.pull) {
                let (named, more) = capped(named);
                readiness.held(Held::Shown(Holding {
                    held_by,
                    repository: row.pull.repository.clone(),
                    number: row.pull.number,
                    url: row.pull.url.clone(),
                    named,
                    more,
                }));
            }
        }
        if withheld.is_empty() {
            return readiness;
        }
        match self.withheld {
            Withheld::Unknown => {
                if readiness.holds.is_empty() {
                    readiness.judged = Judged::Unknown;
                }
            }
            Withheld::Shown => readiness.judged = Judged::Shown,
            Withheld::Masked => {
                for row in withheld {
                    if !self.holds(&row.pull).is_empty() {
                        readiness.held(Held::Private);
                    }
                }
            }
        }
        readiness
    }
}

#[cfg(test)]
mod tests;

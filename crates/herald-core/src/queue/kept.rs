//! A pull request as herald keeps it: folded from its forge's webhooks, and
//! read in full again where a round's listing finds it changed (ADR-0061).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use super::{Checks, Mergeable, Pull, Review, Turn};
use crate::address::Address;
use crate::members::Visibility;
use crate::signal::Validators;

/// The repository a kept pull request is of: the host and port of its
/// pages, and the forge's id of it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RepositoryKey {
    /// The host of its pages, and their port where it is not the scheme's.
    pub host: String,
    /// The forge's id of it.
    pub id: String,
}

impl RepositoryKey {
    /// The repository of id `id` whose pull request's page is `page`, where
    /// the page is a web address.
    #[must_use]
    pub fn of(page: &str, id: &str) -> Option<Self> {
        Address::parse(page).map(|page| Self {
            host: page.authority(),
            id: id.to_owned(),
        })
    }
}

impl fmt::Display for RepositoryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.host, self.id)
    }
}

/// The key a pull request is kept under: its repository and its number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PullKey {
    /// Its repository.
    pub repository: RepositoryKey,
    /// Its number in the repository.
    pub number: u64,
}

impl fmt::Display for PullKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.repository, self.number)
    }
}

/// What a check said last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// It is queued or running.
    Pending,
    /// It passed, or was skipped or neutral.
    Passed,
    /// It failed.
    Failed,
}

/// A check's newest outcome, and the head commit it ran on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ran {
    /// The head commit's sha.
    pub head: String,
    /// What it said.
    pub outcome: Outcome,
    /// When it said so.
    pub at: Timestamp,
}

/// A reviewer's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The reviewer approved it.
    Approved,
    /// The reviewer asked for a change.
    ChangesRequested,
}

/// A reviewer's newest verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reviewed {
    /// The verdict.
    pub verdict: Verdict,
    /// Whether the forge says the reviewer's verdict decides, where the
    /// delivery says.
    pub decides: Option<bool>,
    /// When it was given.
    pub at: Timestamp,
}

/// When each part of a kept pull request's state took its current value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Since {
    /// Whether it is a draft.
    pub draft: Timestamp,
    /// What its reviews say.
    pub review: Timestamp,
    /// What its head's checks say.
    pub checks: Timestamp,
    /// Whether it merges cleanly.
    pub mergeable: Timestamp,
    /// Whether it is in the merge queue.
    pub queued: Timestamp,
    /// Its head commit.
    pub head: Timestamp,
}

impl Since {
    /// Every part taking its value at `at`.
    const fn at(at: Timestamp) -> Self {
        Self {
            draft: at,
            review: at,
            checks: at,
            mergeable: at,
            queued: at,
            head: at,
        }
    }

    /// When each part of `old` took its value, where it became `new` at
    /// `at`.
    fn changed(self, (old, new): (&Pull, &Pull), at: Timestamp) -> Self {
        let when = |same: bool, since: Timestamp| if same { since } else { at };
        Self {
            draft: when(old.draft == new.draft, self.draft),
            review: when(old.review == new.review, self.review),
            checks: when(old.checks == new.checks, self.checks),
            mergeable: when(old.mergeable == new.mergeable, self.mergeable),
            queued: when(old.queued == new.queued, self.queued),
            head: when(old.head == new.head, self.head),
        }
    }
}

/// How a kept pull request closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    /// When it closed, or when herald found it no longer open.
    pub at: Timestamp,
    /// Whether it was merged, where herald knows.
    pub merged: Option<bool>,
}

/// What the last full read of a pull request said of its reviews and its
/// head's checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Read {
    /// When it was read.
    pub at: Timestamp,
    /// Its head commit then.
    pub head: Option<String>,
    /// What its reviews said.
    pub review: Review,
    /// What its head's checks said.
    pub checks: Checks,
}

/// A pull request as herald keeps it (ADR-0061).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptPull {
    /// Its key.
    pub key: PullKey,
    /// What a view reads of it.
    pub pull: Pull,
    /// When each part of its state took its value.
    pub since: Since,
    /// What the last full read said, where it was read in full.
    pub read: Option<Read>,
    /// Each reviewer's newest verdict delivered since that read.
    #[serde(default)]
    pub reviews: BTreeMap<String, Reviewed>,
    /// Each check's newest outcome on its head delivered since that read.
    #[serde(default)]
    pub checks: BTreeMap<String, Ran>,
    /// How it closed, where it did.
    pub closed: Option<Closed>,
}

/// Where a pull request a delivery describes stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// It is open.
    Open,
    /// It closed unmerged.
    Closed,
    /// It was merged.
    Merged,
}

/// A pull request as a webhook delivery describes it, each part it leaves
/// out `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Described {
    /// Its number.
    pub number: u64,
    /// The project it is shown as.
    pub project: String,
    /// Its repository's full path.
    pub repository: String,
    /// Who may see its repository.
    pub visibility: Option<Visibility>,
    /// Its title.
    pub title: String,
    /// Its page.
    pub url: String,
    /// Who opened it, and whether they are a bot.
    pub author: Option<(String, bool)>,
    /// Whether it is a draft.
    pub draft: bool,
    /// Its labels.
    pub labels: Option<Vec<String>>,
    /// Who is asked to review it.
    pub reviewers: Option<Vec<String>>,
    /// Its head commit's sha.
    pub head: Option<String>,
    /// Whether it merges cleanly.
    pub mergeable: Option<Mergeable>,
    /// Whether it is in the merge queue.
    pub queued: Option<bool>,
    /// The state the forge's own status maps to, and the status.
    pub forge: Option<(Turn, String)>,
    /// When it was opened.
    pub opened: Timestamp,
    /// When anyone last acted on it.
    pub updated: Timestamp,
    /// Where it stands.
    pub lifecycle: Lifecycle,
}

/// What a webhook delivery says of a repository's pull requests
/// (ADR-0061).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Folded {
    /// A pull request, as it now stands.
    Pull(Box<Described>),
    /// A reviewer's verdict on one, or its dismissal where `verdict` is
    /// `None`.
    Review {
        /// The pull request's number.
        number: u64,
        /// The reviewer's login.
        reviewer: String,
        /// The verdict.
        verdict: Option<Verdict>,
        /// Whether the forge says the reviewer's verdict decides.
        decides: Option<bool>,
        /// When it was given.
        at: Timestamp,
    },
    /// A check's outcome on a head commit, of every open pull request whose
    /// head it is.
    Check {
        /// The head commit's sha.
        head: String,
        /// The check's name.
        name: String,
        /// What it said.
        outcome: Outcome,
        /// When it said so.
        at: Timestamp,
    },
    /// Someone acted on one without changing its state, as a comment does.
    Activity {
        /// The pull request's number.
        number: u64,
        /// When.
        at: Timestamp,
    },
    /// Who may now see the repository, of every pull request of it kept.
    Visibility(Visibility),
}

/// What a webhook delivery says of one repository's pull requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folding {
    /// The repository.
    pub repository: RepositoryKey,
    /// What it says, in order.
    pub folded: Vec<Folded>,
}

impl Folded {
    /// The number of the pull request it is of, where it names one; a
    /// check is of the pull requests whose head it ran on.
    #[must_use]
    pub const fn number(&self) -> Option<u64> {
        match self {
            Self::Pull(described) => Some(described.number),
            Self::Review { number, .. } | Self::Activity { number, .. } => Some(*number),
            Self::Check { .. } | Self::Visibility(_) => None,
        }
    }
}

/// What the reviews say: a deciding reviewer's change request first, then
/// a verdict whose weight is unknown, then a deciding approval, which
/// cannot tell whether a change request `base` read still stands; else
/// `base`, asked or not as `requested` says.
fn reviewed(base: Review, reviews: &BTreeMap<String, Reviewed>, requested: bool) -> Review {
    let deciding = |verdict: Verdict| {
        reviews
            .values()
            .any(|reviewed| reviewed.decides == Some(true) && reviewed.verdict == verdict)
    };
    if deciding(Verdict::ChangesRequested) {
        return Review::ChangesRequested;
    }
    if reviews.values().any(|reviewed| reviewed.decides.is_none()) {
        return Review::Unknown;
    }
    if deciding(Verdict::Approved) {
        return if base == Review::ChangesRequested {
            Review::Unknown
        } else {
            Review::Approved
        };
    }
    match base {
        Review::Unasked | Review::Requested if requested => Review::Requested,
        Review::Unasked | Review::Requested => Review::Unasked,
        decided => decided,
    }
}

/// What the head's checks say: the failures `base` read that no delivery
/// since spoke of, and every delivered failure; else running where a
/// delivered check runs, or where `base` read some running or failed that
/// have not all spoken; else passed where any delivered check passed; else
/// `base`.
fn checked(base: &Checks, ran: &BTreeMap<String, Ran>) -> Checks {
    if ran.is_empty() {
        return base.clone();
    }
    let mut failed: BTreeSet<&str> = match base {
        Checks::Failed(names) => names
            .iter()
            .map(String::as_str)
            .filter(|name| !ran.contains_key(*name))
            .collect(),
        _ => BTreeSet::new(),
    };
    failed.extend(
        ran.iter()
            .filter(|(_, ran)| ran.outcome == Outcome::Failed)
            .map(|(name, _)| name.as_str()),
    );
    if !failed.is_empty() {
        return Checks::Failed(failed.into_iter().map(ToOwned::to_owned).collect());
    }
    let running = ran.values().any(|ran| ran.outcome == Outcome::Pending);
    if running || matches!(base, Checks::Pending | Checks::Failed(_)) {
        Checks::Pending
    } else {
        Checks::Passed
    }
}

impl KeptPull {
    /// Whether it is open.
    #[must_use]
    pub const fn open(&self) -> bool {
        self.closed.is_none()
    }

    /// `kept`, or a new record keyed `key`, holding `pull` as read at
    /// `now`: each part that changed took its value `now`, and the verdicts
    /// and checks delivered since the last read give way to it. One listed
    /// past the most a view reads in full is kept as unread.
    #[must_use]
    pub fn read(kept: Option<&Self>, key: PullKey, pull: Pull, now: Timestamp) -> Self {
        let since = kept.map_or_else(
            || Since::at(now),
            |kept| kept.since.changed((&kept.pull, &pull), now),
        );
        let read = (!pull.unread).then(|| Read {
            at: now,
            head: pull.head.clone(),
            review: pull.review,
            checks: pull.checks.clone(),
        });
        Self {
            key,
            pull,
            since,
            read,
            reviews: BTreeMap::new(),
            checks: BTreeMap::new(),
            closed: None,
        }
    }

    /// Whether a round's listing of it as `listed` reads it in full again:
    /// it was never read in full, it was closed, its head where the listing
    /// names one, its last activity or its repository differs, or its checks
    /// are still running, which ends with no activity of its own.
    #[must_use]
    pub fn stale(&self, listed: &Pull) -> bool {
        self.read.is_none()
            || !self.open()
            || (listed.head.is_some() && self.pull.head != listed.head)
            || self.pull.updated != listed.updated
            || self.pull.repository != listed.repository
            || self.pull.checks == Checks::Pending
    }

    /// It, seen by `visibility` where that is stricter than who may see it
    /// now: what a delivery or a listing says of who may see a pull
    /// request only ever narrows it; only a full read widens it.
    #[must_use]
    pub fn narrowed(mut self, visibility: Visibility) -> Self {
        self.pull.visibility = self.pull.visibility.max(visibility);
        self
    }

    /// It, closed at `now` as a round's listing no longer lists it open,
    /// where it was open; how it closed is not known.
    #[must_use]
    pub const fn vanished(mut self, now: Timestamp) -> Self {
        if self.closed.is_none() {
            self.closed = Some(Closed {
                at: now,
                merged: None,
            });
        }
        self
    }

    /// `kept`, or a new record keyed `key` where `folded` describes a pull
    /// request herald keeps none of, with `folded` taken in; `None` where
    /// it tells nothing of `kept`.
    #[must_use]
    pub fn fold(kept: Option<Self>, key: PullKey, folded: &Folded) -> Option<Self> {
        match (kept, folded) {
            (None, Folded::Pull(described)) => Some(Self::described(key, described)),
            (Some(mut kept), Folded::Pull(described)) => {
                if described.updated >= kept.pull.updated {
                    kept.describe(described);
                }
                Some(kept)
            }
            (
                Some(mut kept),
                Folded::Review {
                    reviewer,
                    verdict,
                    decides,
                    at,
                    ..
                },
            ) => {
                let newer = kept
                    .reviews
                    .get(reviewer)
                    .is_none_or(|known| known.at <= *at);
                match (newer, verdict) {
                    (false, _) => {}
                    (true, None) => {
                        kept.reviews.remove(reviewer);
                    }
                    (true, Some(verdict)) => {
                        kept.reviews.insert(
                            reviewer.clone(),
                            Reviewed {
                                verdict: *verdict,
                                decides: *decides,
                                at: *at,
                            },
                        );
                    }
                }
                kept.acted(*at);
                kept.refresh(*at);
                Some(kept)
            }
            (
                Some(mut kept),
                Folded::Check {
                    head,
                    name,
                    outcome,
                    at,
                },
            ) => {
                if !kept.open() || kept.pull.head.as_deref() != Some(head.as_str()) {
                    return None;
                }
                if kept.checks.get(name).is_none_or(|known| known.at <= *at) {
                    let ran = Ran {
                        head: head.clone(),
                        outcome: *outcome,
                        at: *at,
                    };
                    kept.checks.insert(name.clone(), ran);
                }
                kept.refresh(*at);
                Some(kept)
            }
            (Some(mut kept), Folded::Activity { at, .. }) => {
                kept.acted(*at);
                Some(kept)
            }
            (Some(kept), Folded::Visibility(visibility)) => Some(kept.narrowed(*visibility)),
            (None, _) => None,
        }
    }

    /// The record `described` begins.
    fn described(key: PullKey, described: &Described) -> Self {
        let pull = Pull {
            project: described.project.clone(),
            repository: described.repository.clone(),
            visibility: Visibility::Private,
            number: described.number,
            title: described.title.clone(),
            url: described.url.clone(),
            author: None,
            bot: false,
            draft: described.draft,
            labels: Vec::new(),
            opened: described.opened,
            updated: described.updated,
            review: Review::Unasked,
            reviewers: Vec::new(),
            checks: Checks::Absent,
            mergeable: Mergeable::Unknown,
            queued: false,
            forge: None,
            head: described.head.clone(),
            unread: false,
        };
        let mut kept = Self {
            key,
            pull,
            since: Since::at(described.updated),
            read: None,
            reviews: BTreeMap::new(),
            checks: BTreeMap::new(),
            closed: None,
        };
        kept.describe(described);
        kept
    }

    /// It, as `described` says it now stands.
    fn describe(&mut self, described: &Described) {
        let at = described.updated;
        let old = self.pull.clone();
        let pull = &mut self.pull;
        pull.title.clone_from(&described.title);
        pull.url.clone_from(&described.url);
        pull.draft = described.draft;
        pull.opened = described.opened;
        pull.updated = at;
        if let Some((author, bot)) = &described.author {
            pull.author = Some(author.clone());
            pull.bot = *bot;
        }
        let parts = [
            (&described.labels, &mut pull.labels),
            (&described.reviewers, &mut pull.reviewers),
        ];
        for (said, kept) in parts {
            if let Some(said) = said {
                kept.clone_from(said);
            }
        }
        if described.head.is_some() && described.head != pull.head {
            pull.head.clone_from(&described.head);
            pull.mergeable = Mergeable::Unknown;
            self.checks.clear();
        }
        pull.mergeable = described.mergeable.unwrap_or(pull.mergeable);
        pull.queued = described.queued.unwrap_or(pull.queued);
        pull.visibility = described
            .visibility
            .map_or(pull.visibility, |said| said.max(pull.visibility));
        if described.forge.is_some() {
            pull.forge.clone_from(&described.forge);
        }
        self.closed = match described.lifecycle {
            Lifecycle::Open => None,
            Lifecycle::Closed | Lifecycle::Merged => Some(Closed {
                at,
                merged: Some(described.lifecycle == Lifecycle::Merged),
            }),
        };
        self.refresh(at);
        self.since = self.since.changed((&old, &self.pull), at);
    }

    /// Its last activity, moved to `at` where that is later.
    fn acted(&mut self, at: Timestamp) {
        self.pull.updated = self.pull.updated.max(at);
    }

    /// Its reviews and checks as the last read and every delivery since
    /// say, each taking its value at `at` where it changed.
    fn refresh(&mut self, at: Timestamp) {
        let old = self.pull.clone();
        let base = self
            .read
            .as_ref()
            .map_or(Review::Unasked, |read| read.review);
        self.pull.review = reviewed(base, &self.reviews, !self.pull.reviewers.is_empty());
        let base = self
            .read
            .as_ref()
            .filter(|read| read.head == self.pull.head)
            .map_or(Checks::Absent, |read| read.checks.clone());
        self.pull.checks = checked(&base, &self.checks);
        self.since = self.since.changed((&old, &self.pull), at);
    }
}

/// Where a source's reconcile stands: the repository it lists, when it
/// last listed it, and the validators each page of the listing was
/// answered with (ADR-0061).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Reconciled {
    /// The repository, where a listing or a delivery named it.
    pub repository: Option<RepositoryKey>,
    /// Each repository of an owner source a listing or a delivery named
    /// (ADR-0062).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub repositories: BTreeSet<RepositoryKey>,
    /// When it was last listed.
    pub at: Option<Timestamp>,
    /// The validators of each page, in order.
    #[serde(default)]
    pub validators: Vec<Validators>,
}

impl Reconciled {
    /// Whether a source whose reconcile stands as `this` is listed again at
    /// `now`, listing at most once `every`, or every round where `None`.
    #[must_use]
    pub fn due(this: Option<&Self>, every: Option<SignedDuration>, now: Timestamp) -> bool {
        match (this.and_then(|this| this.at), every) {
            (Some(at), Some(every)) => at.checked_add(every).is_ok_and(|due| due <= now),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests;

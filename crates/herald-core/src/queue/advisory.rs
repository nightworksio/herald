//! A view's advisories overview: its repositories' open security alerts,
//! shown to an audience only where the one function [`advises`] says the
//! surface it leaves through is private (ADR-0067).

use std::collections::BTreeSet;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::overview::Unread;
use super::{Overviews, Pull, View};
use crate::members::Visibility;
use crate::{DestinationName, RouteId};

/// The most repositories an advisories overview reads one by one where it
/// names no number.
pub const ADVISORY_REPOSITORIES: usize = 20;

/// The most repositories an advisories overview may read one by one.
pub const MOST_ADVISORY_REPOSITORIES: usize = 100;

/// The most open advisories an overview reads where it names no number.
pub const MOST_ADVISORIES: usize = 1000;

/// The most open advisories an overview may be set to read.
pub const MOST_ADVISORIES_ALLOWED: usize = 10_000;

/// The one line an audience that is not private is shown in place of the
/// advisories section, the same for every view and whatever it held.
pub const WITHHELD: &str =
    "A section of this report was withheld from this route because it is not private.";

/// The one line a living post that showed an advisories section is edited
/// to once its route no longer receives one.
pub const RECALLED: &str =
    "The advisory section of this report was removed because this route no longer receives it.";

/// Each place a view's sections can leave herald through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// A route's post, or its living post's next edit: private where the
    /// route itself says `private = true`, and shown the section only where
    /// herald keeps the post where it can take the section out again.
    Route {
        /// Whether the route says `private = true`.
        private: bool,
        /// Whether herald can find the post again to take the section out:
        /// a living post on a destination that edits.
        recallable: bool,
    },
    /// `herald queue`'s table or JSON, or `herald rerun`'s lines: private
    /// as the operator says with `--private` or the config's `[cli] private`.
    Queue {
        /// How private the operator said the output is.
        private: Printed,
    },
    /// A grouped route's digest, which shows no view.
    Digest,
    /// A herald feed or the static log of feeds, which carry the event and
    /// never its view.
    Feed,
    /// A push route, which carries the event alone.
    Push,
    /// A rule's nudge or re-run plan, events of their own with no view.
    Rule,
    /// What a run tells: its lines and a round's answer.
    Told,
    /// Doctor's output, which reads no forge's alerts.
    Doctor,
    /// The app API, which serves no board or report.
    Api,
}

impl Surface {
    /// Every surface, each private one both ways.
    pub const ALL: [Self; 14] = [
        Self::Route {
            private: true,
            recallable: true,
        },
        Self::Route {
            private: true,
            recallable: false,
        },
        Self::Route {
            private: false,
            recallable: true,
        },
        Self::Route {
            private: false,
            recallable: false,
        },
        Self::Queue {
            private: Printed::Asked,
        },
        Self::Queue {
            private: Printed::Configured,
        },
        Self::Queue {
            private: Printed::Withheld,
        },
        Self::Digest,
        Self::Feed,
        Self::Push,
        Self::Rule,
        Self::Told,
        Self::Doctor,
        Self::Api,
    ];
}

/// Whether an advisories section may leave herald through `surface`: only
/// where the surface itself says it is private, and for a route's post only
/// where herald can find the post again to take the section out. No
/// setting and no repository's visibility widens it.
#[must_use]
pub const fn advises(surface: Surface) -> bool {
    match surface {
        Surface::Route {
            private,
            recallable,
        } => private && recallable,
        Surface::Queue { private } => matches!(private, Printed::Asked),
        Surface::Digest
        | Surface::Feed
        | Surface::Push
        | Surface::Rule
        | Surface::Told
        | Surface::Doctor
        | Surface::Api => false,
    }
}

/// Whether a forge's own teams may count toward a load leaving herald
/// through `surface`: who is in an organization's teams is the
/// organization's, and can be secret, whatever repository a row is of, so
/// only where the surface itself says it is private: a route that says
/// `private = true`, and `herald queue` with `--private` or `[cli] private`
/// (ADR-0077).
#[must_use]
pub const fn teams_reach(surface: Surface) -> bool {
    match surface {
        Surface::Route { private, .. } => private,
        Surface::Queue { private } => !matches!(private, Printed::Withheld),
        Surface::Digest
        | Surface::Feed
        | Surface::Push
        | Surface::Rule
        | Surface::Told
        | Surface::Doctor
        | Surface::Api => false,
    }
}

/// Whether a forge's own teams may count toward a load leaving herald
/// through each of `surfaces`, as [`teams_reach`] says of one: never where
/// there is no surface at all, so a load sent nowhere counts none.
#[must_use]
pub fn teams_reach_each(surfaces: impl IntoIterator<Item = Surface>) -> bool {
    let mut surfaces = surfaces.into_iter().peekable();
    surfaces.peek().is_some() && surfaces.all(teams_reach)
}

/// How private the operator said the output of `herald queue`, `herald
/// rerun`, `herald poll` or `herald announce` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Printed {
    /// Said nothing: a private or internal repository's rows are withheld.
    Withheld,
    /// The config's `[cli] private` says it: rows are printed, advisories
    /// are not.
    Configured,
    /// The operator said `--private`: rows and advisories are printed.
    Asked,
}

impl Printed {
    /// How private the output is where the operator `asked` with
    /// `--private` and the config's `[cli] private` is `configured`.
    #[must_use]
    pub const fn of(asked: bool, configured: bool) -> Self {
        if asked {
            Self::Asked
        } else if configured {
            Self::Configured
        } else {
            Self::Withheld
        }
    }

    /// Whether a read that stopped the command is said in full, naming
    /// whatever its forge, its transport or the store named: only where
    /// the operator said the output is private.
    #[must_use]
    pub const fn prints_failures(self) -> bool {
        !matches!(self, Self::Withheld)
    }

    /// Whether the rows of a repository seen as `visibility` are printed:
    /// a public one's always, a private or internal one's only where the
    /// operator said the output is private.
    #[must_use]
    pub const fn prints(self, visibility: Visibility) -> bool {
        matches!(visibility, Visibility::Public) || !matches!(self, Self::Withheld)
    }

    /// `pulls` as they are printed, and those withheld.
    #[must_use]
    pub fn printed(self, pulls: Vec<Pull>) -> (Vec<Pull>, Vec<Pull>) {
        pulls
            .into_iter()
            .partition(|pull| self.prints(pull.visibility))
    }

    /// `overviews` as they are printed beside `shown`, the pull requests it
    /// prints: the CI runs and the unread repositories of those whose rows
    /// it prints, and the heads of `shown` alone.
    #[must_use]
    pub fn overviews(self, mut overviews: Overviews, shown: &[Pull]) -> Overviews {
        if let Some(runs) = overviews.capacity.as_mut() {
            runs.retain(|run| self.prints(run.visibility));
        }
        overviews
            .unread
            .retain(|unread| self.prints(unread.visibility));
        overviews.heads = Overviews::heads_of(shown);
        overviews.header = overviews
            .header
            .map(|header| header.shown(|visibility| self.prints(visibility)));
        overviews
    }
}

/// Which repositories an advisories overview reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvisoriesFrom {
    /// The owner's own listings where its forge has them, else each member
    /// one by one, at most `repositories`.
    Owner,
    /// Those the view shows a pull request of.
    Shown,
    /// The owner's members, the newest pushed first, at most
    /// `repositories`.
    Members,
}

/// How a view reads its advisories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advising {
    /// Which repositories.
    pub from: AdvisoriesFrom,
    /// The most read one by one.
    pub repositories: usize,
    /// The most open advisories read.
    pub most: usize,
}

impl Advising {
    /// How it reads for `view`: a view that holds only some repositories
    /// never reads an owner's own listings, whose pages count the alerts of
    /// the repositories it hides too, but each repository it holds one by
    /// one, at most `repositories` (ADR-0067).
    #[must_use]
    pub fn within(self, view: &View) -> Self {
        match self.from {
            AdvisoriesFrom::Owner if !view.holds_every() => Self {
                from: AdvisoriesFrom::Members,
                ..self
            },
            _ => self,
        }
    }
}

/// One open security alert of a repository: what it is, never what it
/// found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    /// The repository's full path.
    pub repository: String,
    /// What raised it, such as `dependency-scanning` or `secret-scanning`.
    pub tool: String,
    /// How severe the forge says it is; none for a secret.
    pub severity: Option<String>,
    /// What it is: the advisory's summary, the rule's description, or the
    /// secret's type.
    pub summary: Option<String>,
    /// Its state, `open` or `confirmed`.
    pub state: String,
    /// Its page.
    pub url: String,
    /// When it was raised.
    pub opened: Timestamp,
}

/// What an advisories overview read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisories {
    /// The open advisories, at most the view's `most_advisories`.
    pub open: Vec<Advisory>,
    /// What could not be read, and why.
    pub unread: Vec<Unread>,
    /// Whether more were open than it read.
    pub more: bool,
    /// When they were read, which each one's age counts to.
    pub at: Timestamp,
}

impl Advisories {
    /// None read yet, at `at`.
    #[must_use]
    pub const fn none(at: Timestamp) -> Self {
        Self {
            open: Vec::new(),
            unread: Vec::new(),
            more: false,
            at,
        }
    }

    /// It with `other`'s added.
    pub fn merge(&mut self, other: Self) {
        self.open.extend(other.open);
        self.unread.extend(other.unread);
        self.more |= other.more;
    }

    /// It as `view` shows it: only the advisories of the repositories the
    /// view holds, and of what could not be read, a repository `sourced`
    /// names only where the view holds it, an owner or a source always.
    #[must_use]
    pub fn within(mut self, view: &View, sourced: impl Fn(&str) -> bool) -> Self {
        self.open
            .retain(|advisory| view.holds(&advisory.repository));
        self.unread.retain(|unread| unread.within(view, &sourced));
        self
    }
}

/// A living post a route was sent an advisories section in: where it is,
/// never what it showed, so herald can edit the section out once the route
/// no longer receives one (ADR-0067).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisedPost {
    /// The route.
    pub route: RouteId,
    /// Its destination.
    pub destination: DestinationName,
    /// The route's own options for its destination, as its kind writes
    /// them: where the post is, never a secret.
    pub placed: String,
}

/// The most reports [`AdvisedReports`] names: past it a living post is
/// sent the withheld line, never an unrecorded section.
pub const MOST_ADVISED_REPORTS: usize = 10_000;

/// The names of the reports whose cursor may keep a living post an
/// advisories section was sent in, written before the cursor so herald finds
/// each such post again once its report, board or view is gone: names
/// alone, never a route, a place, a count or what was shown (ADR-0067).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisedReports {
    /// The reports' names.
    pub reports: BTreeSet<String>,
}

/// An advisories section as one surface is shown it: what was read, or
/// withheld. Only [`Advised::to`] makes one holding what a forge was read
/// for, so every such section that leaves herald was decided by
/// [`advises`]; [`Advised::withheld`] holds nothing and
/// [`Advised::sample`] one made-up row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advised(Option<Advisories>);

impl Advised {
    /// `found` as `surface` is shown it.
    #[must_use]
    pub fn to(found: &Advisories, surface: Surface) -> Self {
        Self(advises(surface).then(|| found.clone()))
    }

    /// A section withheld, where no advisory was read for it.
    #[must_use]
    pub const fn withheld() -> Self {
        Self(None)
    }

    /// A section holding one made-up advisory at `at`, for a template's
    /// dry render: it holds nothing any forge was read for.
    #[must_use]
    pub fn sample(at: Timestamp) -> Self {
        Self(Some(Advisories {
            open: vec![Advisory {
                repository: "sample-owner/sample-project".to_owned(),
                tool: "dependency-scanning".to_owned(),
                severity: Some("high".to_owned()),
                summary: Some("A sample advisory".to_owned()),
                state: "open".to_owned(),
                url: "https://github.com/sample-owner/sample-project/security/dependabot/1"
                    .to_owned(),
                opened: at,
            }],
            ..Advisories::none(at)
        }))
    }

    /// What it shows, where it was not withheld.
    #[must_use]
    pub const fn shown(&self) -> Option<&Advisories> {
        self.0.as_ref()
    }
}

#[cfg(test)]
mod tests;

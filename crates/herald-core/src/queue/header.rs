//! A release's header: the open milestones its milestone selector names,
//! as each repository holding them describes them, and each repository
//! whose milestones could not be read, with why (ADR-0082).

use jiff::civil::Date;

use super::View;
use super::overview::Unread;
use crate::members::Visibility;

/// The most repositories a release's header reads the milestones of.
pub const MOST_HEADER_REPOSITORIES: usize = 20;

/// The most milestones a release's header names of one repository.
pub const MOST_HEADER_MILESTONES: usize = 10;

/// One open milestone a release's selector names, in one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMilestone {
    /// The repository's full path.
    pub repository: String,
    /// Who may see the repository.
    pub visibility: Visibility,
    /// Its title.
    pub title: String,
    /// Its page, where the forge names one.
    pub url: Option<String>,
    /// Its due day, where it has one.
    pub due: Option<Date>,
    /// Whether its due day ended before the day it was read on, in UTC.
    pub overdue: bool,
    /// How many of its issues are open and closed, as its forge counts
    /// them, where the forge said.
    pub counts: Option<(u32, u32)>,
}

/// A release's header, as one surface is shown it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// The milestones, in the order their repositories were read.
    pub milestones: Vec<OpenMilestone>,
    /// Each repository whose milestones could not be read, or wholly.
    pub unread: Vec<Unread>,
}

impl Header {
    /// It with `other`'s milestones and unread repositories added.
    pub fn merge(&mut self, other: Self) {
        self.milestones.extend(other.milestones);
        self.unread.extend(other.unread);
    }

    /// It with only what `shows`, given a repository's visibility, lets
    /// out.
    #[must_use]
    pub fn shown(&self, shows: impl Fn(Visibility) -> bool) -> Self {
        Self {
            milestones: self
                .milestones
                .iter()
                .filter(|milestone| shows(milestone.visibility))
                .cloned()
                .collect(),
            unread: self
                .unread
                .iter()
                .filter(|unread| shows(unread.visibility))
                .cloned()
                .collect(),
        }
    }

    /// It with only the repositories `view` holds.
    #[must_use]
    pub fn within(mut self, view: &View) -> Self {
        self.milestones
            .retain(|milestone| view.holds(&milestone.repository));
        self.unread.retain(|unread| view.holds(&unread.repository));
        self
    }

    /// Who may see the strictest repository it names.
    #[must_use]
    pub fn strictest(&self) -> Option<Visibility> {
        let milestones = self.milestones.iter().map(|milestone| milestone.visibility);
        let unread = self.unread.iter().map(|unread| unread.visibility);
        milestones.chain(unread).max()
    }
}

#[cfg(test)]
mod tests;

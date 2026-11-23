//! A release's base branches: each branch the pull requests it picks merge
//! into, its head's combined check state read once a round, asked on the
//! validators kept from the last read so an unchanged state is reused
//! (ADR-0083).

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::{Checks, View};
use crate::SourceName;
use crate::members::Visibility;
use crate::signal::Validators;

/// The most base branches a release reads the checks of a round.
pub const MOST_BASES: usize = 20;

/// The most failed checks a kept base branch's state names.
pub const MOST_KEPT_FAILED: usize = 100;

/// The form a kept base branch's state is written in: a record of another
/// form is read again in full.
pub const KEPT_BASE: u32 = 1;

/// What one base branch's head was read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseRead {
    /// Its combined check state.
    Checked(Checks),
    /// It could not be read, for this reason, in plain words.
    Unread(String),
}

/// One base branch of a release, its repository, and what its head's
/// checks were read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Based {
    /// The repository's full path.
    pub repository: String,
    /// Who may see the repository.
    pub visibility: Visibility,
    /// The branch.
    pub branch: String,
    /// What its head's checks were read as.
    pub read: BaseRead,
}

impl Based {
    /// Whether it holds a release back: a check failed on its head, or it
    /// could not be read.
    #[must_use]
    pub const fn holds(&self) -> bool {
        matches!(
            self.read,
            BaseRead::Checked(Checks::Failed(_)) | BaseRead::Unread(_)
        )
    }
}

/// The base branches one surface is told of: those whose repositories it
/// may see, and those withheld from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bases<'a> {
    /// Those it may see.
    pub shown: Vec<&'a Based>,
    /// Those withheld from it.
    pub withheld: Vec<&'a Based>,
}

impl<'a> Bases<'a> {
    /// `bases` parted by whether a surface seeing no repository stricter
    /// than `sees` may see each.
    #[must_use]
    pub fn seen(bases: &'a [Based], sees: Visibility) -> Self {
        let (shown, withheld) = bases.iter().partition(|based| based.visibility <= sees);
        Self { shown, withheld }
    }
}

/// `bases` with only the repositories `view` holds.
pub(super) fn within(bases: &mut Vec<Based>, view: &View) {
    bases.retain(|based| view.holds(&based.repository));
}

/// Which base branch a kept read is of: a source's, of a repository by its
/// full path in lower case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BaseKey {
    /// The source that read it.
    pub source: SourceName,
    /// The repository's full path, in lower case.
    pub repository: String,
    /// The branch.
    pub branch: String,
}

/// One request of a base branch's read, as its forge last answered it in
/// full: the validators it answered with, and what it said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptCheck {
    /// The validators the forge answered with.
    pub validators: Validators,
    /// What the answer said of the head's checks.
    pub checks: Checks,
}

/// A base branch's head as herald last read it, one part per request, kept
/// in the store beside the pull requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptBase {
    /// Which branch it is.
    pub key: BaseKey,
    /// The form it is written in: [`KEPT_BASE`] where this herald wrote it.
    pub form: u32,
    /// When herald last read it.
    pub at: Timestamp,
    /// Each request's last full answer, in the order they are asked.
    pub parts: Vec<KeptCheck>,
}

impl KeptBase {
    /// `parts`, read at `at`, as kept under `key`: none where a part names
    /// more than [`MOST_KEPT_FAILED`] failed checks or carries no
    /// validator to be asked on.
    #[must_use]
    pub fn of(key: BaseKey, parts: Vec<KeptCheck>, at: Timestamp) -> Option<Self> {
        let keeps = |part: &KeptCheck| {
            let validated =
                part.validators.etag.is_some() || part.validators.last_modified.is_some();
            let named = match &part.checks {
                Checks::Failed(names) => names.len(),
                Checks::Absent | Checks::Pending | Checks::Passed => 0,
            };
            validated && named <= MOST_KEPT_FAILED
        };
        (!parts.is_empty() && parts.iter().all(keeps)).then_some(Self {
            key,
            form: KEPT_BASE,
            at,
            parts,
        })
    }

    /// Its parts, where it was written in the form this herald writes and
    /// holds `asked` of them, one for each request a read asks: only then
    /// are its validators asked with.
    #[must_use]
    pub fn fitting(&self, asked: usize) -> Option<&[KeptCheck]> {
        (self.form == KEPT_BASE && self.parts.len() == asked).then_some(self.parts.as_slice())
    }
}

/// What the checks of several requests of one head say together: failed
/// where any failed, each name once; else pending where any runs; else passed
/// where any passed; else none.
#[must_use]
pub fn joined(parts: impl IntoIterator<Item = Checks>) -> Checks {
    let mut failed: Option<Vec<String>> = None;
    let (mut pending, mut passed) = (false, false);
    for checks in parts {
        match checks {
            Checks::Failed(names) => {
                let failed = failed.get_or_insert_default();
                for name in names {
                    if !failed.contains(&name) {
                        failed.push(name);
                    }
                }
            }
            Checks::Pending => pending = true,
            Checks::Passed => passed = true,
            Checks::Absent => {}
        }
    }
    if let Some(failed) = failed {
        Checks::Failed(failed)
    } else if pending {
        Checks::Pending
    } else if passed {
        Checks::Passed
    } else {
        Checks::Absent
    }
}

#[cfg(test)]
mod tests;

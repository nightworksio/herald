//! The keys that choose what a forge source announces: its signals, the
//! pre-release rule, a tag's wait for its Release, and the Releases watched
//! for an edit (ADR-0031, ADR-0032, ADR-0034).

use std::collections::BTreeSet;

use herald_core::{Glob, Prefer, PrereleaseRule, Signal};
use jiff::SignedDuration;

use super::{Sets, SourceOptions, sets_prerelease, sets_signals};
use crate::Forge;

/// What a source announces of one repository, and how it tells a
/// pre-release there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signals {
    /// The signals it announces.
    pub taken: BTreeSet<Signal>,
    /// How it tells a pre-release.
    pub prerelease: PrereleaseRule,
}

impl Default for Signals {
    fn default() -> Self {
        Self {
            taken: Signal::DEFAULT.into(),
            prerelease: PrereleaseRule::default(),
        }
    }
}

impl Signals {
    /// Whether it announces `signal`.
    #[must_use]
    pub fn takes(&self, signal: Signal) -> bool {
        self.taken.contains(&signal)
    }
}

/// How many of the newest Releases are watched for an edit where
/// `edits_depth` is absent.
pub const EDITS_DEPTH: usize = 30;

/// How long a tag waits for its Release where `tag_window` is absent.
pub const TAG_WINDOW: SignedDuration = SignedDuration::from_hours(1);

/// The longest `tag_window` may be.
pub const TAG_WINDOW_ALLOWED: SignedDuration = SignedDuration::from_hours(7 * 24);

impl SourceOptions {
    /// Whether the signals and pre-release rules, the source's and each
    /// override's, are sound on `forge`.
    pub(super) fn signals_check(&self, forge: Forge) -> Result<(), String> {
        let overrides = self.repositories.iter().map(|(glob, chosen)| {
            (
                format!("repositories.\"{glob}\"."),
                &chosen.signals,
                chosen.prerelease,
            )
        });
        for (at, signals, prerelease) in
            std::iter::once((String::new(), &self.signals, self.prerelease)).chain(overrides)
        {
            if signals.as_ref().is_some_and(Vec::is_empty) {
                return Err(format!(
                    "{at}signals names no signal; give release, tag, package or several, or remove it"
                ));
            }
            if forge == Forge::GitLab && prerelease == Some(PrereleaseRule::Flag) {
                return Err(format!(
                    "{at}prerelease \"flag\" marks nothing on GitLab, whose releases carry no pre-release flag; choose either or tag"
                ));
            }
        }
        Ok(())
    }

    /// Whether `prefer` and `tag_window` choose between a tag and its
    /// Release that some repository the source takes announces both of.
    pub(super) fn prefer_check(&self) -> Result<(), String> {
        let both = self.may_take(Signal::Release) && self.may_take(Signal::Tag);
        if let Some(key) = [
            (self.prefer.is_some(), "prefer"),
            (self.tag_window.is_some(), "tag_window"),
        ]
        .into_iter()
        .find_map(|(given, key)| (given && !both).then_some(key))
        {
            return Err(format!(
                "{key} chooses between a tag and its Release, which no repository the source takes announces both of; give signals = [\"release\", \"tag\"], or remove {key}"
            ));
        }
        if self.tag_window() > TAG_WINDOW_ALLOWED {
            return Err("tag_window is at most \"7d\"; shorten it".to_owned());
        }
        if self.tag_window.is_some() && self.prefer() == Prefer::First {
            return Err(
                "tag_window is how long a tag waits for its Release, which prefer = \"first\" never does; remove one".to_owned(),
            );
        }
        Ok(())
    }

    /// Which of a tag and its Release is announced.
    #[must_use]
    pub fn prefer(&self) -> Prefer {
        self.prefer.unwrap_or_default()
    }

    /// How long a tag with no Release waits for one.
    #[must_use]
    pub fn tag_window(&self) -> SignedDuration {
        self.tag_window.map_or(TAG_WINDOW, |window| window.0)
    }

    /// How long a tag of a repository whose signals are `signals` waits for
    /// its Release, where it waits: where they take both and the source
    /// prefers the Release.
    #[must_use]
    pub fn held_for(&self, signals: &Signals) -> Option<SignedDuration> {
        (self.prefer() == Prefer::Release
            && signals.takes(Signal::Release)
            && signals.takes(Signal::Tag))
        .then(|| self.tag_window())
    }

    /// What the source announces of the repository named `name`, and how it
    /// tells a pre-release there: for each, the narrowest of `repositories`
    /// naming it that sets it, or else the source's.
    #[must_use]
    pub fn signals_of(&self, name: &str) -> Signals {
        let narrowest = |sets: Sets| {
            self.repositories
                .iter()
                .filter(|(_, chosen)| sets(chosen))
                .filter_map(|(glob, chosen)| Some((Glob::new(glob).ok()?, chosen)))
                .filter(|(glob, _)| glob.matches(name))
                .max_by_key(|(glob, _)| glob.specificity())
                .map(|(_, chosen)| chosen)
        };
        let signals = narrowest(sets_signals)
            .and_then(|chosen| chosen.signals.as_ref())
            .or(self.signals.as_ref());
        let prerelease = narrowest(sets_prerelease)
            .and_then(|chosen| chosen.prerelease)
            .or(self.prerelease);
        let defaults = Signals::default();
        Signals {
            taken: signals.map_or(defaults.taken, |signals| signals.iter().copied().collect()),
            prerelease: prerelease.unwrap_or(defaults.prerelease),
        }
    }

    /// Whether any repository the source takes may announce `signal`.
    #[must_use]
    pub fn may_take(&self, signal: Signal) -> bool {
        let taken = |signals: &Option<Vec<Signal>>| {
            signals.as_ref().map_or_else(
                || Signals::default().takes(signal),
                |signals| signals.contains(&signal),
            )
        };
        taken(&self.signals)
            || self
                .repositories
                .values()
                .any(|chosen| sets_signals(chosen) && taken(&chosen.signals))
    }

    /// How many of the newest Releases a poll watches for an edit: as many
    /// as `edits_depth` says, at most a page, where a route edits them as
    /// `edits` says, and none otherwise.
    #[must_use]
    pub fn watched(&self, edits: bool) -> usize {
        if edits {
            self.edits_depth
                .unwrap_or(EDITS_DEPTH)
                .min(self.reading().page)
        } else {
            0
        }
    }
}

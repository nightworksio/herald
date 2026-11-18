use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::glob::Glob;
use crate::key::Identity;
use crate::queue::Showing;
use crate::revision::{Edits, LateRelease, Revise, Revision};
use crate::route::Delivery;
use crate::signal::Signal;

/// How long a group gathers its members where `window` is absent.
pub const GROUP_WINDOW: SignedDuration = SignedDuration::from_mins(30);

/// How many members a group takes where `most` is absent.
pub const GROUP_MOST: usize = 25;

/// The most members `most` may let a group take.
pub const GROUP_MOST_ALLOWED: usize = 100;

/// The longest key a group is gathered under, in bytes; an event whose key
/// renders longer is announced alone.
pub const GROUP_KEY_MOST: usize = 256;

/// What the identity of a group's announcement starts with, before its key.
const GROUP_IDENTITY: &str = "group/";

/// How a route announces a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GroupMode {
    /// Hold the members until the window passes, then announce them as one.
    #[default]
    Hold,
    /// Announce the first member at once, and edit its post as each other
    /// member comes within the window.
    Edit,
}

/// When a held group is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GroupUntil {
    /// When its window passes.
    #[default]
    Window,
    /// When the member that leads it comes, or its window passes.
    Lead,
}

/// What a route does with a member that comes after its group was
/// announced: once a held group closed to be announced, or once an edited
/// group's window passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GroupLate {
    /// Announce it alone.
    #[default]
    Post,
    /// Edit the group's post to take it, or announce it alone where the
    /// destination edits no post.
    Edit,
    /// Leave it unannounced.
    Skip,
}

/// How a route gathers the events that share a key into one announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grouping {
    /// The template of the key events are grouped by.
    pub key: String,
    /// How long a group gathers its members, from when herald sees the
    /// first.
    pub window: SignedDuration,
    /// The projects whose event leads a group, the first that matches a
    /// member first.
    pub lead: Vec<Glob>,
    /// How the group is announced.
    pub mode: GroupMode,
    /// When a held group is announced.
    pub until: GroupUntil,
    /// What becomes of a member that comes after its group was announced.
    pub late: GroupLate,
    /// How many members a group takes; each member more is announced alone.
    pub most: usize,
}

impl Grouping {
    /// Whether `event` leads a group by what `lead` names.
    #[must_use]
    pub fn leads(&self, event: &Event) -> bool {
        self.lead.iter().any(|glob| glob.matches(event.project()))
    }

    /// The members of a group, `events` in the order they came, with the
    /// one that leads it first: the first member of the project the first
    /// matching glob of `lead` names, or the first member where none does.
    #[must_use]
    pub fn ordered<'a>(&self, events: &'a [Event]) -> Vec<&'a Event> {
        let led = self
            .lead
            .iter()
            .find_map(|glob| {
                events
                    .iter()
                    .position(|event| glob.matches(event.project()))
            })
            .unwrap_or_default();
        events
            .get(led)
            .into_iter()
            .chain(
                events
                    .iter()
                    .enumerate()
                    .filter(|(at, _)| *at != led)
                    .map(|(_, event)| event),
            )
            .collect()
    }
}

/// The identity of the announcement a route makes of the group `key`.
#[must_use]
pub fn group_identity(key: &str) -> Identity {
    Identity::new(format!("{GROUP_IDENTITY}{key}"))
}

/// A group of events announced as one: its key and its members, the lead
/// first.
#[derive(Debug, Clone, Copy)]
pub struct Group<'a> {
    /// The key its members share.
    pub key: &'a str,
    /// Its members, the lead first.
    pub events: &'a [&'a Event],
}

/// What a member's ledger entry records it showed in its group's
/// announcement: its signal and a digest of its title and body.
#[must_use]
pub fn shown(event: &Event) -> Revision {
    Revision::of(event.signal(), [event.title(), event.body()])
}

/// How a group's announcement comes to show other than it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A member joins it after it landed.
    Joined,
    /// A member it shows as a tag comes as its Release.
    Released,
    /// A member's Release is edited.
    Edited,
}

impl Change {
    /// How `event` changes the member a group's announcement shows as
    /// `was`; none where it shows it as it is, or where nothing of it a
    /// route edits for changed.
    #[must_use]
    pub fn of(was: &Revision, event: &Event) -> Option<Self> {
        if shown(event).digest == was.digest {
            None
        } else if was.signal == Some(Signal::Tag) && event.signal() == Some(Signal::Release) {
            Some(Self::Released)
        } else if event.edited() {
            Some(Self::Edited)
        } else {
            None
        }
    }

    /// What a route delivering as `delivery`, on a destination that edits
    /// where `edits` says, does to a group's post this changes: a member
    /// joining edits it, or is announced alone where the post cannot be
    /// edited; a Release of a tag member as the route's `late_release`
    /// says; an edited Release as its `edits` says.
    #[must_use]
    pub fn revise(self, delivery: &Delivery, edits: bool) -> Revise {
        match (self, delivery.late_release) {
            (Self::Joined, _) | (Self::Released, LateRelease::Edit) if edits => Revise::Edit,
            (Self::Joined, _) | (Self::Released, LateRelease::Edit | LateRelease::Post) => {
                Revise::Post
            }
            (Self::Edited, _) if edits && delivery.edits == Edits::Release => Revise::Edit,
            _ => Revise::Keep,
        }
    }
}

/// What an announcement is made of: an event, and the group it leads where
/// its route gathers events into one.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// The event, or the group's lead.
    pub event: &'a Event,
    /// The group it leads, where it leads one.
    pub group: Option<Group<'a>>,
    /// The view it reports, where it is a report.
    pub view: Option<Showing<'a>>,
}

impl Subject<'_> {
    /// The identity its announcement is keyed by: the group's, or the
    /// event's.
    #[must_use]
    pub fn identity(&self) -> Identity {
        self.group.map_or_else(
            || self.event.identity().clone(),
            |group| group_identity(group.key),
        )
    }
}

impl<'a> From<&'a Event> for Subject<'a> {
    fn from(event: &'a Event) -> Self {
        Self {
            event,
            group: None,
            view: None,
        }
    }
}

#[cfg(test)]
mod tests;

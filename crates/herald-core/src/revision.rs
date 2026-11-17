//! What a post already announced shows, and how a route brings it to an
//! announcement that changed: a tag's post to its Release, or a Release's to
//! its edited notes (ADR-0034).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::declined::Declined;
use crate::key::digest;
use crate::ledger::{Entry, Step};
use crate::route::Delivery;
use crate::signal::Signal;
use crate::status;

/// Why a part of a post cannot be edited or deleted: its message is not
/// recorded.
pub const NO_PART: Declined = Declined::Missing("the message of the part");

/// Whether `step`, answered `status`, found its message already gone: a
/// deletion answered not found or gone, which leaves what it would.
#[must_use]
pub const fn already_deleted(step: &Step, status: u16) -> bool {
    matches!(step, Step::Delete(_)) && matches!(status, status::NOT_FOUND | status::GONE)
}

/// What an announcement shows: the signal of the event it announced and a
/// digest of what the route rendered of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    /// The signal of the event announced, where it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<Signal>,
    /// The SHA-256, in lowercase hex, of every part the route rendered.
    pub digest: String,
}

impl Revision {
    /// The revision of `parts`, rendered of an event of `signal`.
    #[must_use]
    pub fn of<'a>(signal: Option<Signal>, parts: impl IntoIterator<Item = &'a str>) -> Self {
        let rendered: Vec<String> = parts
            .into_iter()
            .map(|part| format!("{}\n{part}", part.len()))
            .collect();
        Self {
            signal,
            digest: digest(&rendered.concat()),
        }
    }
}

/// What a route does with a Release whose tag it already announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LateRelease {
    /// Edit the tag's post to the Release, or post the Release where the
    /// destination cannot edit.
    #[default]
    Edit,
    /// Post the Release as an announcement of its own.
    Post,
    /// Leave the tag's post as it is.
    Skip,
}

/// Which announcements a route edits the post of, when the forge changes
/// what it announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Edits {
    /// None: a post stays as it was first announced.
    #[default]
    None,
    /// A Release whose name or notes were edited.
    Release,
}

/// What a route does with an announcement whose release the forge deleted
/// (ADR-0048).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Withdraw {
    /// Keep its post.
    #[default]
    Keep,
    /// Delete every message herald recorded of it, where the destination
    /// can.
    Delete,
}

/// What a route does to an announcement whose post landed, when what it
/// renders has changed since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Revise {
    /// Leave the post as it is.
    Keep,
    /// Edit the post, and its chunks, to the new revision.
    Edit,
    /// Post the new revision as an announcement of its own.
    Post,
}

/// What a route delivering as `delivery` does to the post `entry` keeps,
/// now rendered as `revision`, on a destination that edits where `edits`
/// says: a tag's post brought to its Release as its `late_release` says, and
/// a Release's to its edited notes where its `edits` takes them.
#[must_use]
pub fn revise(entry: &Entry, revision: &Revision, delivery: &Delivery, edits: bool) -> Revise {
    let Some(shown) = entry.revision.as_ref() else {
        return Revise::Keep;
    };
    if shown.digest == revision.digest || entry.message(&Step::Post).is_none() {
        return Revise::Keep;
    }
    match (shown.signal, revision.signal, delivery.late_release) {
        (Some(Signal::Tag), Some(Signal::Release), LateRelease::Edit) if edits => Revise::Edit,
        (Some(Signal::Tag), Some(Signal::Release), LateRelease::Edit | LateRelease::Post) => {
            Revise::Post
        }
        (Some(Signal::Release), Some(Signal::Release), _)
            if edits && delivery.edits == Edits::Release =>
        {
            Revise::Edit
        }
        _ => Revise::Keep,
    }
}

/// What a route does to the post `entry` keeps of a group, now rendered as
/// `revision`, on a destination that edits where `edits` says: edits it
/// where it landed and the group renders otherwise, and leaves it
/// otherwise.
#[must_use]
pub fn regrouped(entry: &Entry, revision: &Revision, edits: bool) -> Revise {
    match &entry.revision {
        Some(shown)
            if edits && shown.digest != revision.digest && entry.message(&Step::Post).is_some() =>
        {
            Revise::Edit
        }
        _ => Revise::Keep,
    }
}

/// The steps that bring the post `entry` keeps to `revision`, rendered in
/// `parts` parts: the post and each chunk that landed edited, each chunk
/// more posted, each chunk fewer deleted, the thread opened from the post
/// renamed, and the revision delivered to the followers the deliveries
/// reached.
#[must_use]
pub fn revised(entry: &Entry, revision: &Revision, parts: u32) -> Vec<Step> {
    let edit = |part| Step::Edit {
        revision: revision.digest.clone(),
        part,
    };
    let landed = entry.chunks();
    let mut steps = vec![edit(0)];
    steps.extend((1..parts).map(|part| {
        if landed.contains(&part) {
            edit(part)
        } else {
            Step::Chunk(part)
        }
    }));
    steps.extend(
        landed
            .iter()
            .filter(|part| **part >= parts)
            .map(|part| Step::Delete(*part)),
    );
    if entry.facts(&Step::OpenThread).is_some() {
        steps.push(Step::Rename(revision.digest.clone()));
    }
    if entry.facts(&Step::Deliveries).is_some() {
        steps.push(Step::Updates(revision.digest.clone()));
    }
    steps
}

#[cfg(test)]
mod tests;

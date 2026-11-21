//! What a post already announced shows, and how a route brings it to an
//! announcement that changed: a tag's post to its Release, or a Release's to
//! its edited notes (ADR-0034).

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::declined::Declined;
use crate::key::digest;
use crate::ledger::{Entry, State, Step};
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

/// A living post of a view that reads advisories deleted and posted anew in
/// place of an edit, where its destination keeps what an edit replaced: the
/// revision it posts, when it began, and the parts of the earlier post it
/// still deletes, the post itself first and then its chunks last first
/// (ADR-0067).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacing {
    /// What the post it posts shows.
    pub revision: Revision,
    /// When it began.
    pub at: Timestamp,
    /// The parts of the earlier post still to delete.
    pub deleting: Vec<u32>,
}

impl Replacing {
    /// The deletions still to take.
    #[must_use]
    pub fn deletions(&self) -> Vec<Step> {
        self.deleting.iter().copied().map(Step::Delete).collect()
    }

    /// What its post and chunks are told apart by: the revision and when
    /// the replacement began, so no post a destination took under an earlier
    /// replacement, even of the same revision, is taken for this one's.
    #[must_use]
    pub fn token(&self) -> String {
        format!("{}.{}", self.revision.digest, self.at.as_second())
    }
}

/// A part of a post a run sent and never found again, and the key it was
/// sent under: it may still show what it showed, until a send under the
/// same key lands or the operator says it was deleted (ADR-0067).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stray {
    /// The part: 0 for the post, else the chunk.
    pub part: u32,
    /// The transaction key or `Idempotency-Key` it was sent under.
    pub token: String,
}

/// Whether a replacement of the post `entry` keeps by one showing
/// `revision` resumes the one under way: it posts the same revision.
#[must_use]
pub fn resumes(entry: &Entry, revision: &Revision) -> bool {
    entry
        .replacing
        .as_ref()
        .is_some_and(|under_way| under_way.revision.digest == revision.digest)
}

/// The replacement of the post `entry` keeps by one showing `revision`,
/// begun at `at`: the one under way where it [`resumes`] it, so a run that
/// stopped part way goes on, and else every part `entry` holds to delete,
/// the post first and then its chunks last first, so the part every reader
/// sees goes before any other. A new replacement begins over an entry
/// holding no step only attempted ([`Entry::found`]).
#[must_use]
pub fn replaced(entry: &Entry, revision: &Revision, at: Timestamp) -> Replacing {
    match &entry.replacing {
        Some(under_way) if resumes(entry, revision) => under_way.clone(),
        _ => {
            let mut deleting: Vec<u32> =
                entry.message(&Step::Post).map(|_| 0).into_iter().collect();
            deleting.extend(entry.chunks().into_iter().rev());
            Replacing {
                revision: revision.clone(),
                at,
                deleting,
            }
        }
    }
}

impl Entry {
    /// When `step` was last sent, where its answer is not recorded.
    #[must_use]
    pub fn sent_at(&self, step: &Step) -> Option<Timestamp> {
        match self.state(step) {
            Some(State::Attempted { at }) => Some(*at),
            _ => None,
        }
    }

    /// What its post is told apart by where the destination takes a post
    /// once: its key and the replacement under way's token where one is,
    /// else its key and when the post was sent, so no post of an entry
    /// housekeeping forgot is taken for this one's.
    #[must_use]
    pub fn post_token(&self) -> String {
        match (&self.replacing, self.sent_at(&Step::Post)) {
            (Some(replacing), _) => format!("{}.{}", self.key, replacing.token()),
            (None, Some(at)) => format!("{}.{}", self.key, at.as_second()),
            (None, None) => self.key.to_string(),
        }
    }

    /// What a destination that takes a post once tells the overflow chunk
    /// `index` apart by: the key and the index, and the replacement under
    /// way's token where one is, else the revision the entry brings its
    /// post to where it records one and when the chunk was sent, so a chunk
    /// a later revision, or a later post, sends again is not taken for one
    /// deleted before.
    #[must_use]
    pub fn chunk_token(&self, index: u32) -> String {
        let shown = match (&self.replacing, &self.revision) {
            (Some(replacing), _) => {
                return format!("{}.{index}.{}", self.key, replacing.token());
            }
            (None, Some(revision)) => format!(".{}", revision.digest),
            (None, None) => String::new(),
        };
        let sent = self
            .sent_at(&Step::Chunk(index))
            .map(|at| format!(".{}", at.as_second()))
            .unwrap_or_default();
        format!("{}.{index}{shown}{sent}", self.key)
    }

    /// Its post and each chunk it sent and never recorded landing.
    #[must_use]
    pub fn unrecorded(&self) -> Vec<Step> {
        self.records
            .iter()
            .filter(|record| matches!(record.state, State::Attempted { .. }))
            .filter(|record| matches!(record.step, Step::Post | Step::Chunk(_)))
            .map(|record| record.step.clone())
            .collect()
    }

    /// The entry a replacement begins over: each of its post and chunks only
    /// attempted landed as `found`, read back from the destination, shows
    /// it, so the replacement deletes it, and else kept among its strays,
    /// so no replacement takes a post an earlier one sent for its own.
    #[must_use]
    pub fn found(self, found: Option<&Self>) -> Self {
        let unrecorded = self.unrecorded();
        unrecorded.into_iter().fold(self, |entry, step| {
            match found.and_then(|found| found.facts(&step)).cloned() {
                Some(facts) => entry.landed(step, facts),
                None => entry.strayed(&step),
            }
        })
    }

    /// The part `step` sends, with the key it goes under now: the post's
    /// or a chunk's, none for any other step.
    fn keyed(&self, step: &Step) -> Option<Stray> {
        match step {
            Step::Post => Some(Stray {
                part: 0,
                token: self.post_token(),
            }),
            Step::Chunk(part) => Some(Stray {
                part: *part,
                token: self.chunk_token(*part),
            }),
            _ => None,
        }
    }

    /// The entry with `step`, its post or a chunk sent and never found
    /// again, kept among its strays with the key it was sent under, and its
    /// record dropped.
    #[must_use]
    pub fn strayed(mut self, step: &Step) -> Self {
        let Some(stray) = self.keyed(step) else {
            return self;
        };
        self.records.retain(|record| record.step != *step);
        if !self.strays.contains(&stray) {
            self.strays.push(stray);
        }
        self
    }

    /// The entry with `step`, its post or a chunk, declined outright on its
    /// first send: its attempt dropped, as the destination made nothing a
    /// later run could find again.
    #[must_use]
    pub fn refused(mut self, step: &Step) -> Self {
        if matches!(step, Step::Post | Step::Chunk(_)) {
            self.records.retain(|record| {
                !(record.step == *step && matches!(record.state, State::Attempted { .. }))
            });
        }
        self
    }

    /// The entry no longer holding as a stray the part `step` sends under
    /// the key it goes under now, as that send landed.
    #[must_use]
    pub fn unstrayed(mut self, step: &Step) -> Self {
        if let Some(sent) = self.keyed(step) {
            self.strays.retain(|stray| *stray != sent);
        }
        self
    }

    /// The entry as its replacement plans the post it posts anew: every
    /// part that landed and is still to delete dropped, so it is posted
    /// again, and every step only attempted kept, so it is read back
    /// before it is sent again.
    #[must_use]
    pub fn reposting(&self) -> Self {
        let deleting: &[u32] = self
            .replacing
            .as_ref()
            .map_or(&[], |replacing| &replacing.deleting);
        let mut entry = self.clone();
        entry.records.retain(|record| {
            let part = match record.step {
                Step::Post => Some(0),
                Step::Chunk(part) => Some(part),
                _ => None,
            };
            !(matches!(record.state, State::Landed { .. })
                && part.is_some_and(|part| deleting.contains(&part)))
        });
        entry
    }

    /// The entry once every step of its replacement landed: showing the
    /// revision it posted, with no replacement under way.
    #[must_use]
    pub fn settled(mut self) -> Self {
        if let Some(replacing) = self.replacing.take() {
            self.revision = Some(replacing.revision);
        }
        self
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

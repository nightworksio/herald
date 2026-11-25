//! What a forge source gives a merge queue on one of its repositories, or
//! why it takes none as asked (ADR-0087).

use std::collections::BTreeMap;
use std::fmt;

use herald_core::merging::{Check, Enter, Message, Method, Update, Updated, Vouches, Write};
use herald_core::{Permission, Rerun, Visibility};
use herald_ports::{Boxed, DynHttp};

use crate::Credential;
use jiff::{SignedDuration, Timestamp};

use crate::{Plan, Unpolled};

/// What a merge queue asks of the source it names.
#[derive(Debug, Clone, Copy)]
pub struct MergeAsked<'a> {
    /// The repository it merges in, where it names one.
    pub repository: Option<&'a str>,
    /// How it merges, where it says.
    pub method: Option<Method>,
    /// Whether it re-runs failed checks.
    pub retries: bool,
    /// The credential its steps are made with, where it names one.
    pub write: Option<&'a Write>,
}

/// What a merge queue's read asks of the gestures that queue its pull
/// requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnterAsked<'a> {
    /// The gesture that queues a pull request.
    pub enter: &'a Enter,
    /// The least role the person who queues holds.
    pub least: Permission,
    /// Which head a gesture vouches for.
    pub vouches: Vouches,
    /// How far apart the forge's stamps may be: a push stamped up to this
    /// before a gesture or an approval counts as after it.
    pub skew: SignedDuration,
    /// Whether the gesture vouches for the head herald's own update made:
    /// approvals are read where it asks for one.
    pub updated: Updated,
}

/// Why a source takes no merge queue as asked: the queue's key at fault,
/// and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmerged {
    /// The key, as the queue's table writes it.
    pub key: &'static str,
    /// Why, with the fix.
    pub why: String,
}

/// What a forge source gives a merge queue on one of its repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merging {
    /// The source's kind, as the config names it.
    pub forge: &'static str,
    /// The repository's full path.
    pub repository: String,
    /// The base URL of the forge's API, the one origin a step goes to.
    pub api: String,
    /// How a pull request's branch is brought up to date.
    pub update: Update,
    /// Why the forge has no merge queue of its own, where it never has.
    pub queueless: Option<&'static str>,
    /// The least-privilege read credential its reads take.
    pub reads: &'static str,
}

/// What a merge queue reads of the base branch it merges into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branched {
    /// Who may see the repository.
    pub visibility: Visibility,
    /// The branch's head commit.
    pub head: String,
    /// The checks the forge requires before a merge into it.
    pub required: Vec<String>,
    /// Whether the forge has a merge queue or merge train on it; none
    /// where the forge would not say.
    pub queue: Option<bool>,
    /// Whether every listing its required checks come from was read
    /// whole.
    pub whole: bool,
}

/// A gesture that queued a pull request, as the forge records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gesture {
    /// The forge's id of the label event or comment it is, where the forge
    /// names one: no two gestures share it.
    pub id: Option<u64>,
    /// Who made it, as the forge records its actor.
    pub by: String,
    /// When the forge recorded it.
    pub at: Timestamp,
    /// The head the pull request had then, by the forge's own record of
    /// pushes; none where it holds none.
    pub head: Option<String>,
    /// The commit its comment named by its full id, where it named one.
    pub named: Option<String>,
    /// The role `by` holds on the repository now; none below write, or
    /// where the forge would not say.
    pub role: Option<Permission>,
}

/// An open pull request into a queue's branch that a gesture stands on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entered {
    /// Its number.
    pub number: u64,
    /// Its page.
    pub url: String,
    /// The id the forge's write calls name it by where that is not its
    /// number: GitHub's node id.
    pub id: Option<String>,
    /// Whether it is a draft.
    pub draft: bool,
    /// Its head commit.
    pub head: String,
    /// Its title.
    pub title: String,
    /// Its description; empty where it has none.
    pub body: String,
    /// The gesture that queued it.
    pub gesture: Gesture,
    /// When its head arrived, by the forge's record of pushes, where it
    /// holds one.
    pub arrived: Option<Timestamp>,
    /// Whether one who holds the queue's least role approved its head on
    /// the forge; read only where the queue's gesture vouches for an
    /// approved head.
    pub approved: bool,
    /// Whether the records its gesture and its approval were judged from
    /// were read whole: one judged from part of them never stands.
    pub whole: bool,
}

/// What proves a queued pull request green on its base, as the forge says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proved {
    /// Whether its head holds the base's head.
    pub up_to_date: bool,
    /// The checks reported on its head.
    pub checks: Vec<Check>,
    /// The checks the forge requires of it, by the names its checks carry.
    pub required: Vec<String>,
    /// Whether the forge says it can merge; none while the forge works it
    /// out.
    pub mergeable: Option<bool>,
    /// Whether the forge says its head merges into the base's head without
    /// a conflict; none while the forge works it out.
    pub clean: Option<bool>,
    /// Whether every check on its head, and every check its base
    /// requires, was read: never green where one was not.
    pub whole: bool,
}

/// What reads a merge queue's repository on its forge, with the source's
/// read credential alone (ADR-0087).
pub trait MergeReader: fmt::Debug + Send + Sync {
    /// The base branch `branch`, asked through `http`.
    fn branch<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        branch: &'a str,
    ) -> Boxed<'a, Result<Branched, Unpolled>>;

    /// The open pull requests into `branch` that a gesture `asked` names
    /// queued, oldest gesture first, at most
    /// [`herald_core::merging::MOST_ENTERED`], asked through `http`, each
    /// one's approval read where `asked` vouches for an approved head.
    fn entered<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        branch: &'a str,
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Vec<Entered>, Unpolled>>;

    /// What proves the pull request `entered` green on the base `branched`,
    /// asked through `http`.
    fn proved<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        entered: &'a Entered,
        branched: &'a Branched,
    ) -> Boxed<'a, Result<Proved, Unpolled>>;

    /// The pull request `pull` names by its base branch and number, read
    /// again on its own as [`MergeReader::entered`] reads each one, with
    /// nothing kept from an earlier read; none where it is not open into
    /// that branch or no gesture `asked` names stands on it. Asked through
    /// `http`.
    fn entry<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        pull: (&'a str, u64),
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Option<Entered>, Unpolled>>;

    /// Whether the pull request `number` is in the forge's merge queue or
    /// merge train, or set to join it, asked through `http`.
    fn queued<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        number: u64,
    ) -> Boxed<'a, Result<bool, Unpolled>>;

    /// The open pull requests into `branch` in the forge's merge queue or
    /// merge train, or set to join it, on which a gesture `asked` names was
    /// made, with every mark on each as [`TakenBack`] says, at most
    /// [`herald_core::merging::MOST_ENTERED`], asked through `http`.
    fn taken_back<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        branch: &'a str,
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Vec<TakenBack>, Unpolled>>;

    /// The pull request `pull` names by its base branch and number, read
    /// again on its own as [`MergeReader::taken_back`] reads each one, with
    /// nothing kept from an earlier read; none where it is not open into
    /// that branch or no gesture was made on it. Asked
    /// through `http`.
    fn took_back<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        pull: (&'a str, u64),
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Option<TakenBack>, Unpolled>>;
    /// Whether the pull request `number` is merged, asked through `http`.
    fn merged<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        number: u64,
    ) -> Boxed<'a, Result<bool, Unpolled>>;

    /// The parents of the commit `sha`, in order, where the forge itself
    /// made and signed it, as it signs a commit made through its own API
    /// for the committer `signer`, or on github.com its own where `signer`
    /// is none; none where it did not, or where the forge never says, asked
    /// through `http`.
    fn forge_made<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        made: (&'a str, Option<&'a str>),
    ) -> Boxed<'a, Result<Option<Vec<String>>, Unpolled>>;

    /// The methods the repository allows a merge by, asked through `http`.
    fn methods<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Result<Methods, Unpolled>>;

    /// The newest failed runs on the head of `failed` whose job of its
    /// check failed, each with that job and the exact call re-running it
    /// alone, or why none can be re-run, asked through `http` (ADR-0068).
    fn failed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        failed: (&'a str, &'a str),
    ) -> Boxed<'a, Result<Plan, Unpolled>>;
}

/// The methods a repository allows a merge by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Methods {
    /// Each method it allows, never `repository`.
    pub allowed: Vec<Method>,
    /// Whether the forge said: none is known where the read credential may
    /// not see the repository's settings.
    pub whole: bool,
}

/// How a pull request is merged: the method, never `repository`, and the
/// commit message where herald gives one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Merge<'a> {
    /// The method.
    pub method: Method,
    /// The commit message, where herald gives one.
    pub message: Message,
    /// The pull request as read right before the merge.
    pub entered: &'a Entered,
}

/// What the forge answered a step of herald's own queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// It took the step.
    Took,
    /// It could not bring the branch up to date: it conflicts with the
    /// base, as it says.
    Conflict(String),
    /// It refused, saying this, with no credential in it.
    Refused(String),
    /// Its answer says neither, or was lost.
    Lost,
}

/// A mark the forge records on a pull request: the id of the label event
/// or comment it is, where the forge names one, who made it, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marked {
    /// The forge's id of the label event or comment.
    pub id: Option<u64>,
    /// Who made it, as the forge records its actor; empty where the forge
    /// names none, as for a deleted account.
    pub by: String,
    /// When the forge recorded it.
    pub at: Timestamp,
}

/// The role a person holds on the repository now, as the forge says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holds {
    /// This role, write or above.
    Role(Permission),
    /// None at write or above, as the forge says, or the account is gone.
    Nothing,
    /// The forge would not say.
    Unsaid,
}

impl Holds {
    /// The role held; none below write or where the forge would not say.
    #[must_use]
    pub const fn role(self) -> Option<Permission> {
        match self {
            Self::Role(role) => Some(role),
            Self::Nothing | Self::Unsaid => None,
        }
    }
}

/// An open pull request into a queue's branch, in the forge's merge queue
/// or merge train, on which a gesture was made: every mark on it, whoever
/// made it, for the queue to judge against the gesture herald handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenBack {
    /// Its number.
    pub number: u64,
    /// Its page.
    pub url: String,
    /// The id the forge's write calls name it by where that is not its
    /// number: GitHub's node id.
    pub id: Option<String>,
    /// Every gesture made on it, whoever made it, newest first.
    pub gestures: Vec<Marked>,
    /// Every cancel made on it, whoever made it, newest first.
    pub cancels: Vec<Marked>,
    /// The role each maker of `gestures` and `cancels` the forge names
    /// holds on the repository now, by who made it.
    pub makers: BTreeMap<String, Holds>,
    /// Whether its marks were read whole: a pull request judged from part
    /// of them is never taken out.
    pub whole: bool,
}

impl TakenBack {
    /// The role `by` holds on the repository now, as read with it; none
    /// below write, where the forge would not say, or names no one.
    #[must_use]
    pub fn role_of(&self, by: &str) -> Option<Permission> {
        self.makers.get(by).copied().and_then(Holds::role)
    }
}

/// What the forge answered a write call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    /// It did what it was asked: took the pull request into its queue, or
    /// set it to join, or took it out.
    Done,
    /// It refused, saying this, with no credential in it.
    Refused(String),
    /// Its answer says neither, or was lost.
    Lost,
}

/// What hands a pull request to its forge's merge queue or merge train
/// with a queue's write credential, sent to the forge's API alone
/// (ADR-0087). Made only with that credential, which only the queue's own
/// apply reads.
pub trait MergeWriter: fmt::Debug + Send + Sync {
    /// The base URL the write credential is sent to: the forge's API.
    fn api(&self) -> &str;

    /// `entered` handed to the forge's queue with its head as the head the
    /// forge must find, sent through `write`, reaching the API alone; what
    /// the forge answered, or why nothing was sent.
    fn hand_off<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        entered: &'a Entered,
    ) -> Boxed<'a, Result<Answered, Unpolled>>;

    /// `taken` taken out of the forge's queue, sent through `write`,
    /// reaching the API alone; what the forge answered, or why nothing was
    /// sent.
    fn take_out<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        taken: &'a TakenBack,
    ) -> Boxed<'a, Result<Answered, Unpolled>>;

    /// `entered`'s branch brought up to date with its base as `update`
    /// says, its head named as the head the forge must find where the forge
    /// takes one, sent through `write`; what the forge answered, or why
    /// nothing was sent.
    fn update<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        entered: &'a Entered,
        update: Update,
    ) -> Boxed<'a, Result<Done, Unpolled>>;

    /// The pull request `merge` names merged as it says, its head named as
    /// the head the forge must find, sent through `write`; what the forge
    /// answered, or why nothing was sent.
    fn merge<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        merge: Merge<'a>,
    ) -> Boxed<'a, Result<Done, Unpolled>>;

    /// The failed job `job` re-run alone through ADR-0068's narrowest
    /// call, sent through `write`; what the forge answered, or why nothing
    /// was sent.
    fn rerun<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        job: &'a Rerun,
    ) -> Boxed<'a, Result<Done, Unpolled>>;

    /// Whether the forge takes the head a branch must have when it brings
    /// it up to date: where it does not, herald reads the head again right
    /// before the call.
    fn pins_update(&self) -> bool;
}

/// The credential a merge queue's step is made with, read by its apply
/// alone, and the forge it is sent to.
#[derive(Debug, Clone)]
pub struct WriteWith<'a> {
    /// What the source gives the queue: the forge, repository and API.
    pub merging: &'a Merging,
    /// The credential.
    pub credential: Credential,
}

#[cfg(test)]
mod tests;

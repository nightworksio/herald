//! What a forge source gives a merge queue on one of its repositories, or
//! why it takes none as asked (ADR-0087).

use std::fmt;

use herald_core::merging::{Check, Enter, Method, Update, Vouches, Write};
use herald_core::{Permission, Visibility};
use herald_ports::{Boxed, DynHttp};

use crate::Credential;
use jiff::{SignedDuration, Timestamp};

use crate::Unpolled;

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
}

/// What the forge answered a hand-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handed {
    /// It took the pull request into its queue, or set it to join.
    Queued,
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
    ) -> Boxed<'a, Result<Handed, Unpolled>>;
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

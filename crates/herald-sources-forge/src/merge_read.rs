//! A merge queue's reads on its forge with the source's read credential:
//! the base branch, each open pull request a gesture queued with who made
//! it, their role now and the head it vouches for by the forge's own record
//! of pushes, and what proves it green (ADR-0087).

use std::collections::BTreeMap;

use herald_core::merging::{Enter, MOST_ENTERED, Updated, Vouches};
use herald_core::{MOST_RUNS, Permission};
use herald_kinds::{
    Branched, EnterAsked, Entered, Gesture, Holds, Marked, MergeReader, Methods, Plan, Proved,
    TakenBack, Unpolled,
};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Request};
use jiff::{SignedDuration, Timestamp};
use serde::de::DeserializeOwned;

use crate::limited::{checked, limited};
use crate::{Forge, Origin, Token};

mod forgejo;
mod github;
mod gitlab;
mod owning;

/// The word that, after a queue's comment command, takes a pull request
/// out.
const CANCEL: &str = "cancel";

/// The review state that approves, as GitHub and Forgejo name it.
const APPROVED: &str = "APPROVED";

/// The lengths of a commit's full id: SHA-1's and SHA-256's.
const FULL_IDS: [usize; 2] = [40, 64];

/// The most pushes, comments or label events one pull request's read
/// takes.
const MOST_MARKS: usize = 500;

/// The most open pull requests into a branch one read lists.
const MOST_OPENED: usize = 500;

/// What a pull request read on its own is called in an error.
const PULL: &str = "the pull request";

/// The state of an open pull request on GitHub and Forgejo.
const OPEN: &str = "open";

/// What a listing of open pull requests is called in an error.
const OPENED: &str = "open pull requests";

/// What reads a merge queue's repository on its forge.
#[derive(Debug)]
pub(crate) struct ForgeMergeReader {
    /// The repository, on its forge.
    pub(crate) origin: Origin,
    /// The source's read credential, where it has one.
    pub(crate) token: Option<Token>,
    /// The forge's GraphQL endpoint, beside its API.
    pub(crate) graphql: String,
}

/// Who did something, as the forge records its actor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Actor {
    login: String,
    id: Option<u64>,
}

/// Who made a label mark: the actor the forge names, or, where it names
/// none, as for a deleted account, an unnamed one whose taking the label
/// off still counts and who never queues.
fn label_actor(by: Option<Actor>, into: bool) -> Option<Actor> {
    by.or_else(|| {
        (!into).then(|| Actor {
            login: String::new(),
            id: None,
        })
    })
}

/// An open pull request into the branch, as the forge lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Opened {
    number: u64,
    title: String,
    body: String,
    url: String,
    id: Option<String>,
    draft: bool,
    head: String,
    author: Option<String>,
    labels: Vec<String>,
    head_repository: Option<String>,
    head_branch: Option<String>,
}

/// A mark on a pull request: a gesture that puts it in, or one that takes
/// it out.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mark {
    id: Option<u64>,
    by: Actor,
    at: Timestamp,
    into: bool,
    named: Option<String>,
}

/// A review of a pull request, as the forge records it: by whom, on which
/// commit, and whether it approves (true), asks for changes or was
/// dismissed (false), or neither.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Review {
    by: Actor,
    commit: Option<String>,
    approves: Option<bool>,
}

/// The reviews of a pull request the forge listed, oldest first, and
/// whether it listed every one.
type Reviews = (Vec<Review>, bool);

/// The gesture that stands on a pull request, and whether its record was
/// read whole.
type Stood = (Mark, bool);

/// Every mark on a pull request, whether they were read whole, and
/// whether they are label marks.
type Marks = (Vec<Mark>, bool, bool);

/// A comment, as the forge records it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Said {
    id: Option<u64>,
    by: Actor,
    at: Timestamp,
    body: String,
    edited: bool,
}

/// A push to a pull request's head, as the forge records it: when, and the
/// head it moved from where the forge says.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Push {
    at: Timestamp,
    before: Option<String>,
}

/// The pushes to a pull request's head the forge listed, and whether it
/// listed every one.
type Pushes = (Vec<Push>, bool);

/// The least-privilege read credential each forge's merge queue reads
/// with.
pub(crate) const fn scopes(forge: Forge) -> &'static str {
    match forge {
        Forge::GitHub => {
            "a GitHub App or fine-grained token with Metadata, Contents, Pull requests, Checks and Commit statuses read on the repository (and on a fork's, for its pushes), on GitHub Enterprise Server and GHE.com too; a classic token needs repo for a private repository"
        }
        Forge::GitLab => {
            "a token with read_api, of the Reporter role or above, on GitLab Self-Managed and Dedicated too"
        }
        Forge::Forgejo => {
            "a token with read:repository and read:issue, of a user who administers the repository: Forgejo and Gitea say another person's role to an administrator alone"
        }
    }
}

/// Whether `text` is a commit's full id: 40 or 64 lowercase hex digits.
fn full_id(text: &str) -> bool {
    FULL_IDS.contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether the comment `body` is the command `command` putting a pull
/// request in, alone or naming the commit it vouches for by its full id,
/// its command followed by `cancel` taking it out, or neither.
fn commanded(command: &str, body: &str) -> Option<(bool, Option<String>)> {
    let body = body.trim();
    if body == command.trim() {
        return Some((true, None));
    }
    let rest = body.strip_prefix(command.trim())?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    match rest.trim() {
        CANCEL => Some((false, None)),
        id if full_id(id) => Some((true, Some(id.to_owned()))),
        _ => None,
    }
}

/// The gesture that stands after the counted `marks`: the last that put
/// the pull request in, unless a later mark took it out.
fn standing(mut marks: Vec<Mark>) -> Option<Mark> {
    marks.sort_by_key(|mark| mark.at);
    marks.pop().filter(|mark| mark.into)
}

/// `mark` as the forge records it.
fn marked(mark: Mark) -> Marked {
    Marked {
        id: mark.id,
        by: mark.by.login,
        at: mark.at,
    }
}

/// Those whose last review in `reviews` that approves or asks for changes
/// approves the commit `head`.
fn approving(reviews: Vec<Review>, head: &str) -> Vec<Actor> {
    let mut last: Vec<Review> = Vec::new();
    for review in reviews
        .into_iter()
        .filter(|review| review.approves.is_some())
    {
        last.retain(|held| held.by.login != review.by.login);
        last.push(review);
    }
    last.into_iter()
        .filter(|review| review.approves == Some(true) && review.commit.as_deref() == Some(head))
        .map(|review| review.by)
        .collect()
}

/// The head a pull request whose head is `head` had at `at`, by `pushes`,
/// the forge's record of pushes to it, `whole` where it listed every one,
/// a push stamped up to `skew` before `at` counted as after it: its
/// head now where none came after, otherwise the head the first of those
/// moved from; none where the record does not say.
fn head_then(
    (pushes, whole): &Pushes,
    (at, skew): (Timestamp, SignedDuration),
    head: &str,
) -> Option<String> {
    let at = at.checked_sub(skew).unwrap_or(Timestamp::MIN);
    let first_after = pushes
        .iter()
        .filter(|push| push.at >= at)
        .min_by_key(|push| push.at);
    let reaches = *whole || pushes.iter().any(|push| push.at < at);
    match first_after {
        None if reaches => Some(head.to_owned()),
        Some(push) if reaches => push.before.clone(),
        _ => None,
    }
}

/// When the last of `pushes` came, where any did.
fn arrived(pushes: &[Push]) -> Option<Timestamp> {
    pushes.iter().map(|push| push.at).max()
}

/// The marks of the comments `said` under the command `command`: an
/// edited comment counts only where it takes a pull request out.
fn comment_marks(said: Vec<Said>, command: &str) -> Vec<Mark> {
    said.into_iter()
        .filter_map(|said| {
            commanded(command, &said.body)
                .filter(|(into, _)| !(said.edited && *into))
                .map(|(into, named)| Mark {
                    id: said.id,
                    by: said.by,
                    at: said.at,
                    into,
                    named,
                })
        })
        .collect()
}

impl ForgeMergeReader {
    /// The forge.
    const fn forge(&self) -> Forge {
        self.origin.forge()
    }

    /// The request for `path` under the repository.
    fn get(&self, path: &str) -> Result<Request, Unpolled> {
        self.origin.get(path, self.token.as_ref())
    }

    /// The request for `path` under the API's base URL.
    fn rooted(&self, path: &str) -> Request {
        self.origin.rooted(path, self.token.as_ref())
    }

    /// The answer to `request`, sent through `http`, read as `what`; none
    /// where the forge says nothing is there.
    async fn read<T: DeserializeOwned>(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        request: Request,
        what: &'static str,
    ) -> Result<Option<T>, Unpolled> {
        let forge = self.forge();
        let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
        if let Some(limited) = limited(forge, what, &answer) {
            return Err(limited);
        }
        match answer.status {
            404 => return Ok(None),
            401 | 403 => {
                return Err(Unpolled::Unreadable(format!(
                    "the forge refused {what}: a merge queue reads with {}",
                    scopes(forge)
                )));
            }
            _ => {}
        }
        checked(forge, what, &answer)?;
        herald_wire::read(what, self.origin.reading().parse, &answer.body)
            .map(Some)
            .map_err(Unpolled::unreadable)
    }

    /// The answer to `request`, read as `what`, where it must be there.
    async fn needed<T: DeserializeOwned>(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        request: Request,
        what: &'static str,
    ) -> Result<T, Unpolled> {
        self.read(http, request, what)
            .await?
            .ok_or_else(|| Unpolled::Unreadable(format!("the forge found no {what}")))
    }

    /// Each item of the listing `ask` makes the request for, at most
    /// `most`, read as `what`, with whether it was read whole.
    ///
    /// # Errors
    ///
    /// Where the forge refuses it, or it could not be read.
    async fn paged<T: DeserializeOwned>(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        ask: &(dyn Fn(&str) -> Result<Request, Unpolled> + Sync),
        bound: (usize, &'static str),
    ) -> Result<(Vec<T>, bool), Unpolled> {
        crate::paging::paged(http, self.forge(), ask, bound)
            .await?
            .ok_or_else(|| {
                Unpolled::Unreadable(format!(
                    "the forge refused {}: a merge queue reads with {}",
                    bound.1,
                    scopes(self.forge())
                ))
            })
    }

    /// Each open pull request into the branch the listing `ask` makes the
    /// request for reads.
    ///
    /// # Errors
    ///
    /// Where the forge refuses it, or it holds more than [`MOST_OPENED`],
    /// so a gesture past them would go unseen.
    async fn opened<T: DeserializeOwned>(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        ask: &(dyn Fn(&str) -> Result<Request, Unpolled> + Sync),
    ) -> Result<Vec<T>, Unpolled> {
        let (opened, whole) = self.paged(http, ask, (MOST_OPENED, OPENED)).await?;
        if !whole {
            return Err(Unpolled::Unreadable(format!(
                "more than {MOST_OPENED} pull requests are open, and herald reads no more"
            )));
        }
        Ok(opened)
    }

    /// The role `actor` holds on the repository now, read once a read.
    async fn role(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        actor: &Actor,
        roles: &mut BTreeMap<String, Holds>,
    ) -> Result<Holds, Unpolled> {
        if let Some(role) = roles.get(&actor.login) {
            return Ok(*role);
        }
        let role = match self.forge() {
            Forge::GitHub => github::role(self, http, actor).await?,
            Forge::GitLab => gitlab::role(self, http, actor).await?,
            Forge::Forgejo => forgejo::role(self, http, actor).await?,
        };
        roles.insert(actor.login.clone(), role);
        Ok(role)
    }

    /// Every mark on `pull` under `enter`, whoever made it, with whether
    /// they were read whole and whether they are label marks.
    async fn marks(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &Opened,
        enter: &Enter,
    ) -> Result<Marks, Unpolled> {
        let ((marks, whole), labelled) = match enter {
            Enter::Label(label) => {
                let marks = match self.forge() {
                    Forge::GitHub => github::labelled(self, http, pull.number, label).await?,
                    Forge::GitLab => gitlab::labelled(self, http, pull.number, label).await?,
                    Forge::Forgejo => forgejo::labelled(self, http, pull.number, label).await?,
                };
                (marks, true)
            }
            Enter::Comment(command) => {
                let (said, whole) = match self.forge() {
                    Forge::GitHub => github::commented(self, http, pull.number).await?,
                    Forge::GitLab => gitlab::commented(self, http, pull.number).await?,
                    Forge::Forgejo => forgejo::commented(self, http, pull.number).await?,
                };
                ((comment_marks(said, command), whole), false)
            }
        };
        Ok((marks, whole, labelled))
    }

    /// Those of `marks` on `pull` that count: a gesture made by one who
    /// holds `least` or above, a label taken off by anyone, a cancel
    /// comment by the author or one who holds a role.
    async fn counted(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &Opened,
        (marks, labelled, least): (Vec<Mark>, bool, Permission),
        roles: &mut BTreeMap<String, Holds>,
    ) -> Result<Vec<Mark>, Unpolled> {
        let mut counted = Vec::new();
        for mark in marks {
            let free = labelled || pull.author.as_deref() == Some(mark.by.login.as_str());
            let counts = if mark.into {
                self.role(http, &mark.by, roles)
                    .await?
                    .role()
                    .is_some_and(|role| role >= least)
            } else {
                free || self.role(http, &mark.by, roles).await?.role().is_some()
            };
            if counts {
                counted.push(mark);
            }
        }
        Ok(counted)
    }

    /// The gesture that stands on `pull` under `enter`, made by one who
    /// holds `least` or above, by who made it and when, with whether its
    /// record was read whole.
    async fn gestured(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &Opened,
        (enter, least): (&Enter, Permission),
        roles: &mut BTreeMap<String, Holds>,
    ) -> Result<Option<Stood>, Unpolled> {
        let (marks, whole, labelled) = self.marks(http, pull, enter).await?;
        let counted = self
            .counted(http, pull, (marks, labelled, least), roles)
            .await?;
        Ok(standing(counted).map(|mark| (mark, whole)))
    }

    /// `pull` under the gesture `asked` names, with every mark on it,
    /// newest first, and the role of each maker the forge names read into
    /// `roles`; none where no gesture was made on it.
    async fn taken_of(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: Opened,
        asked: EnterAsked<'_>,
        roles: &mut BTreeMap<String, Holds>,
    ) -> Result<Option<TakenBack>, Unpolled> {
        let (mut marks, whole, _) = self.marks(http, &pull, asked.enter).await?;
        if !marks.iter().any(|mark| mark.into) {
            return Ok(None);
        }
        let mut makers = BTreeMap::new();
        for mark in &marks {
            if !mark.by.login.is_empty() {
                let held = self.role(http, &mark.by, roles).await?;
                makers.insert(mark.by.login.clone(), held);
            }
        }
        marks.sort_by_key(|mark| mark.at);
        marks.reverse();
        let (gestures, cancels): (Vec<Mark>, Vec<Mark>) =
            marks.into_iter().partition(|mark| mark.into);
        Ok(Some(TakenBack {
            number: pull.number,
            url: pull.url,
            id: pull.id,
            gestures: gestures.into_iter().map(marked).collect(),
            cancels: cancels.into_iter().map(marked).collect(),
            makers,
            whole,
        }))
    }

    /// The pull request `number`, where it is open into `branch`.
    async fn opened_one(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (branch, number): (&str, u64),
    ) -> Result<Option<Opened>, Unpolled> {
        match self.forge() {
            Forge::GitHub => github::one(self, http, (branch, number)).await,
            Forge::GitLab => gitlab::one(self, http, (branch, number)).await,
            Forge::Forgejo => forgejo::one(self, http, (branch, number)).await,
        }
    }

    /// Each open pull request into `branch` in the forge's queue that may
    /// be taken out of it, under the gesture `asked` names.
    async fn taken(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        branch: &str,
        asked: EnterAsked<'_>,
    ) -> Result<Vec<TakenBack>, Unpolled> {
        let mut numbers = match self.forge() {
            Forge::GitHub => github::in_queue(self, http, branch).await?,
            Forge::GitLab => gitlab::in_queue(self, http, branch).await?,
            Forge::Forgejo => Vec::new(),
        };
        numbers.sort_unstable();
        numbers.dedup();
        if numbers.len() > MOST_ENTERED {
            return Err(Unpolled::Unreadable(format!(
                "more than {MOST_ENTERED} pull requests are in the forge's queue on the branch, and herald reads no more"
            )));
        }
        let mut roles = BTreeMap::new();
        let mut taken = Vec::new();
        for number in numbers {
            let Some(pull) = self.opened_one(http, (branch, number)).await? else {
                continue;
            };
            if let Some(back) = self.taken_of(http, pull, asked, &mut roles).await? {
                taken.push(back);
            }
        }
        Ok(taken)
    }

    /// Whether one who holds `least` or above approved `pull`'s head, with
    /// whether the forge's reviews were read whole.
    async fn approved(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &Opened,
        (least, skew): (Permission, SignedDuration),
        roles: &mut BTreeMap<String, Holds>,
    ) -> Result<(bool, bool), Unpolled> {
        let (reviews, whole) = match self.forge() {
            Forge::GitHub => github::reviews(self, http, pull.number).await?,
            Forge::GitLab => gitlab::reviews(self, http, (pull, skew)).await?,
            Forge::Forgejo => forgejo::reviews(self, http, pull.number).await?,
        };
        for by in approving(reviews, &pull.head) {
            if self
                .role(http, &by, roles)
                .await?
                .role()
                .is_some_and(|role| role >= least)
            {
                return Ok((true, whole));
            }
        }
        Ok((false, whole))
    }

    /// The pushes the forge records to `pull`'s head.
    async fn pushes(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &Opened,
    ) -> Result<Pushes, Unpolled> {
        match self.forge() {
            Forge::GitHub => github::pushes(self, http, pull).await,
            Forge::GitLab => gitlab::pushes(self, http, pull.number).await,
            Forge::Forgejo => forgejo::pushes(self, http, pull.number).await,
        }
    }

    /// `pull` as an entry, where a gesture `asked` names stands on it, each
    /// role read once into `roles`.
    async fn entry_of(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: Opened,
        asked: EnterAsked<'_>,
        roles: &mut BTreeMap<String, Holds>,
    ) -> Result<Option<Entered>, Unpolled> {
        let EnterAsked {
            enter,
            least,
            vouches,
            skew,
            updated,
        } = asked;
        let Some((mark, whole)) = self.gestured(http, &pull, (enter, least), roles).await? else {
            return Ok(None);
        };
        let Mark {
            id, by, at, named, ..
        } = mark;
        let role = self.role(http, &by, roles).await?.role();
        let pushes = self.pushes(http, &pull).await?;
        let (approved, reviewed) = if vouches == Vouches::Approved || updated == Updated::Approved {
            self.approved(http, &pull, (least, skew), roles).await?
        } else {
            (false, true)
        };
        Ok(Some(Entered {
            gesture: Gesture {
                id,
                head: head_then(&pushes, (at, skew), &pull.head),
                named,
                by: by.login,
                at,
                role,
            },
            arrived: arrived(&pushes.0),
            approved,
            whole: whole && reviewed,
            number: pull.number,
            title: pull.title,
            body: pull.body,
            url: pull.url,
            id: pull.id,
            draft: pull.draft,
            head: pull.head,
        }))
    }

    /// The pull request `number`, read again on its own with a role read
    /// afresh for each person, where it is open into `branch`, holds the
    /// label `asked` names where it names one, and a gesture stands on it.
    async fn one(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (branch, number): (&str, u64),
        asked: EnterAsked<'_>,
    ) -> Result<Option<Entered>, Unpolled> {
        let pull = self.opened_one(http, (branch, number)).await?;
        let Some(pull) = pull.filter(|pull| match asked.enter {
            Enter::Label(label) => pull.labels.iter().any(|held| held == label),
            Enter::Comment(_) => true,
        }) else {
            return Ok(None);
        };
        self.entry_of(http, pull, asked, &mut BTreeMap::new()).await
    }

    /// Each open pull request into `branch` a gesture `asked` names stands
    /// on, oldest gesture first.
    async fn entries(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        branch: &str,
        asked: EnterAsked<'_>,
    ) -> Result<Vec<Entered>, Unpolled> {
        let label = match asked.enter {
            Enter::Label(label) => Some(label.as_str()),
            Enter::Comment(_) => None,
        };
        let opened = match self.forge() {
            Forge::GitHub => github::opened(self, http, branch).await?,
            Forge::GitLab => gitlab::opened(self, http, branch).await?,
            Forge::Forgejo => forgejo::opened(self, http, branch).await?,
        };
        let candidates: Vec<Opened> = opened
            .into_iter()
            .filter(|pull| label.is_none_or(|label| pull.labels.iter().any(|held| held == label)))
            .collect();
        if candidates.len() > MOST_ENTERED {
            return Err(Unpolled::Unreadable(format!(
                "more than {MOST_ENTERED} open pull requests into the branch may hold a gesture, and herald reads no more"
            )));
        }
        let mut roles = BTreeMap::new();
        let mut entries = Vec::new();
        for pull in candidates {
            if let Some(entry) = self.entry_of(http, pull, asked, &mut roles).await? {
                entries.push(entry);
            }
        }
        entries.sort_by_key(|entry| (entry.gesture.at, entry.number));
        Ok(entries)
    }
}

impl MergeReader for ForgeMergeReader {
    fn branch<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        branch: &'a str,
    ) -> Boxed<'a, Result<Branched, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            match self.forge() {
                Forge::GitHub => github::branch(self, &http, branch).await,
                Forge::GitLab => gitlab::branch(self, &http, branch).await,
                Forge::Forgejo => forgejo::branch(self, &http, branch).await,
            }
        })
    }

    fn entered<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        branch: &'a str,
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Vec<Entered>, Unpolled>> {
        Box::pin(async move { self.entries(&Erased(http), branch, asked).await })
    }

    fn proved<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        entered: &'a Entered,
        branched: &'a Branched,
    ) -> Boxed<'a, Result<Proved, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            match self.forge() {
                Forge::GitHub => github::proved(self, &http, entered, branched).await,
                Forge::GitLab => gitlab::proved(self, &http, entered, branched).await,
                Forge::Forgejo => forgejo::proved(self, &http, entered, branched).await,
            }
        })
    }

    fn entry<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (branch, number): (&'a str, u64),
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Option<Entered>, Unpolled>> {
        Box::pin(async move { self.one(&Erased(http), (branch, number), asked).await })
    }

    fn queued<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        number: u64,
    ) -> Boxed<'a, Result<bool, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            match self.forge() {
                Forge::GitHub => github::queued(self, &http, number).await,
                Forge::GitLab => gitlab::queued(self, &http, number).await,
                Forge::Forgejo => Ok(false),
            }
        })
    }

    fn taken_back<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        branch: &'a str,
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Vec<TakenBack>, Unpolled>> {
        Box::pin(async move { self.taken(&Erased(http), branch, asked).await })
    }

    fn took_back<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        pull: (&'a str, u64),
        asked: EnterAsked<'a>,
    ) -> Boxed<'a, Result<Option<TakenBack>, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            let Some(opened) = self.opened_one(&http, pull).await? else {
                return Ok(None);
            };
            self.taken_of(&http, opened, asked, &mut BTreeMap::new())
                .await
        })
    }

    fn merged<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        number: u64,
    ) -> Boxed<'a, Result<bool, Unpolled>> {
        Box::pin(async move { owning::merged(self, &Erased(http), number).await })
    }

    fn forge_made<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        made: (&'a str, Option<&'a str>),
    ) -> Boxed<'a, Result<Option<Vec<String>>, Unpolled>> {
        Box::pin(async move { owning::forge_made(self, &Erased(http), made).await })
    }

    fn methods<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Result<Methods, Unpolled>> {
        Box::pin(async move { owning::methods(self, &Erased(http)).await })
    }

    fn failed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        failed: (&'a str, &'a str),
    ) -> Boxed<'a, Result<Plan, Unpolled>> {
        Box::pin(async move {
            let read = (&self.origin, self.token.as_ref());
            crate::queue::planned(read, &Erased(http), failed, MOST_RUNS).await
        })
    }
}

#[cfg(test)]
mod tests;

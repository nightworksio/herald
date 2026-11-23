//! What a forge's webhook deliveries say of a repository's pull requests,
//! folded into the records kept mode keeps (ADR-0061).

use herald_core::{Folded, Folding, Outcome, RepositoryKey, Verdict};
use herald_wire::SourceError;

use crate::Forge;
use crate::listing::Named;

mod github;
mod gitlab;

/// What a delivery kept mode folds is read as, on each forge.
const FOLDED: Named = Named {
    github: "GitHub's delivery of a pull request's change",
    gitlab: "GitLab's delivery of a merge request's change",
    forgejo: "Forgejo's delivery of a pull request's change",
};

/// What a delivery kept mode folds is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// A pull request's opening, change or closing.
    Pull,
    /// A review given or dismissed: GitHub's, or Forgejo's approval or
    /// rejection, the latter `approved` where it approves.
    Review(Option<bool>),
    /// A comment on a pull request's review, or a merge request's note.
    Remark,
    /// A comment on an issue, which is a pull request where it says so.
    Comment,
    /// A check run's or a pipeline's outcome.
    Check,
    /// A commit status's outcome.
    Status,
    /// A change of the repository itself, such as who may see it.
    Repository,
    /// A merge group of GitHub's merge queue, whose checks were asked for
    /// or which was destroyed (ADR-0078).
    MergeGroup,
}

impl Delivery {
    /// What the event named `name` on `forge` is, where kept mode folds
    /// it.
    pub(crate) fn named(forge: Forge, name: &str) -> Option<Self> {
        match (forge, name) {
            (Forge::GitLab, "Merge Request Hook") | (_, "pull_request") => Some(Self::Pull),
            (Forge::GitHub, "pull_request_review") => Some(Self::Review(None)),
            (Forge::Forgejo, "pull_request_approved") => Some(Self::Review(Some(true))),
            (Forge::Forgejo, "pull_request_rejected") => Some(Self::Review(Some(false))),
            (Forge::GitLab, "Note Hook")
            | (
                Forge::GitHub | Forge::Forgejo,
                "pull_request_review_comment" | "pull_request_comment",
            ) => Some(Self::Remark),
            (Forge::GitHub | Forge::Forgejo, "issue_comment") => Some(Self::Comment),
            (Forge::GitLab, "Pipeline Hook") | (Forge::GitHub, "check_run") => Some(Self::Check),
            (Forge::GitHub | Forge::Forgejo, "status") => Some(Self::Status),
            (Forge::GitHub | Forge::Forgejo, "repository") => Some(Self::Repository),
            (Forge::GitHub, "merge_group") => Some(Self::MergeGroup),
            _ => None,
        }
    }
}

/// What a review delivery says of its reviewer's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    /// The reviewer gave this verdict.
    Gave(Verdict),
    /// The reviewer's verdict was dismissed or withdrawn.
    Withdrew,
}

impl Said {
    /// The verdict standing once it was said, none where it was withdrawn.
    const fn verdict(self) -> Option<Verdict> {
        match self {
            Self::Gave(verdict) => Some(verdict),
            Self::Withdrew => None,
        }
    }
}

/// What a delivery says, read: the repository's full path, its key, and
/// what it says of its pull requests.
pub(crate) type Read = (String, RepositoryKey, Vec<Folded>);

/// What a source whose deliveries are folded is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Of<'a> {
    /// One repository, by its full path.
    Repository(&'a str),
    /// An owner's repositories, a GitLab group's including its subgroups'
    /// where it says so (ADR-0062).
    Owner(&'a str, bool),
}

/// What the delivery `payload` of the event named `name` on `forge` says
/// of the pull requests of a repository `of` holds, shown as its name:
/// nothing where kept mode folds no such event, or the delivery says
/// nothing.
///
/// # Errors
///
/// When the payload is not the event it is named, or is of a repository
/// `of` does not hold.
pub(crate) fn folded(
    forge: Forge,
    (name, payload): (&str, &[u8]),
    of: Of<'_>,
) -> Result<Option<Folding>, SourceError> {
    let Some(delivery) = Delivery::named(forge, name) else {
        return Ok(None);
    };
    let what = FOLDED.on(forge);
    let (path, key, mut folded) = match forge {
        Forge::GitLab => gitlab::folded(what, delivery, payload, "")?,
        Forge::GitHub | Forge::Forgejo => github::folded(what, delivery, payload, "")?,
    };
    for said in &mut folded {
        if let Folded::Pull(described) = said {
            crate::kind::named(&path).clone_into(&mut described.project);
        }
    }
    let (held, holder) = match of {
        Of::Repository(repository) => (path.eq_ignore_ascii_case(repository), repository),
        Of::Owner(owner, subgroups) => (
            path.rsplit_once('/')
                .is_some_and(|(holder, _)| forge.holds((owner, subgroups), holder)),
            owner,
        ),
    };
    if !held {
        return Err(SourceError::Unreadable {
            what,
            reason: format!("it is not of a repository {holder}, what its source names, holds"),
        });
    }
    Ok((!folded.is_empty()).then_some(Folding {
        repository: key,
        folded,
    }))
}

/// What a check that ran to `conclusion`, or still runs where there is
/// none, said.
fn concluded(conclusion: Option<&str>) -> Outcome {
    match conclusion {
        None => Outcome::Pending,
        Some(conclusion) if crate::queue::failed(conclusion) => Outcome::Failed,
        Some(_) => Outcome::Passed,
    }
}

/// The key of the repository of id `id` whose page is `page`.
fn keyed(what: &'static str, page: &str, id: u64) -> Result<RepositoryKey, SourceError> {
    RepositoryKey::of(page, &id.to_string()).ok_or_else(|| SourceError::Unreadable {
        what,
        reason: format!("its repository's page {page} is not a web address"),
    })
}

#[cfg(test)]
mod tests;

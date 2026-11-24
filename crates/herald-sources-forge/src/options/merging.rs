//! What a forge source gives a merge queue on one of its repositories, and
//! how it brings a branch up to date (ADR-0087).

use herald_core::merging::{Method, Update};
use herald_core::path_of_names;
use herald_kinds::{MergeAsked, Merging, Unmerged};

use super::SourceOptions;
use super::write::GITHUB_APPS;
use crate::Forge;
use crate::forge::api_of;

/// Why a GitLab source takes no `update = "merge"`.
pub const GITLAB_UPDATES: &str = "GitLab brings a merge request up to date by rebasing it alone; give update = \"rebase\", or remove update";

/// Why a GitHub queue takes no fast-forward.
pub const GITHUB_FAST_FORWARD: &str = "GitHub has no fast-forward merge; give method = \"merge\", \"squash\", \"rebase\" or \"repository\"";

/// Why a Forgejo queue takes no `retries`.
pub const FORGEJO_RETRIES: &str =
    "Forgejo has no API to re-run a job, so a Forgejo queue takes no retries; remove retries";

/// Why Forgejo has no merge queue of its own.
pub const FORGEJO_QUEUELESS: &str = "Forgejo and Gitea have no merge queue";

impl SourceOptions {
    /// Whether `update` is one `forge` offers.
    pub(super) fn update_check(&self, forge: Forge) -> Result<(), String> {
        if forge == Forge::GitLab && self.update == Some(Update::Merge) {
            return Err(GITLAB_UPDATES.to_owned());
        }
        Ok(())
    }

    /// How a merge queue on `forge` brings a branch up to date.
    #[must_use]
    pub fn updating(&self, forge: Forge) -> Update {
        self.update.unwrap_or(match forge {
            Forge::GitLab => Update::Rebase,
            Forge::GitHub | Forge::Forgejo => Update::Merge,
        })
    }

    /// The repository a merge queue asking for `named` merges in: the
    /// source's own, or one under the owner it names.
    fn merged_in(&self, forge: Forge, named: Option<&str>) -> Result<String, String> {
        let owner = self.owner.as_deref().unwrap_or_default();
        match (&self.repository, named) {
            (Some(own), None) => Ok(own.clone()),
            (Some(own), Some(_)) => Err(format!(
                "the source is the one repository {own}; remove repository"
            )),
            (None, None) => Err(format!(
                "the source is the owner {owner}; name the repository the queue merges in, as {}",
                forge.shape()
            )),
            (None, Some(named))
                if path_of_names(named, forge.names())
                    && named
                        .strip_prefix(owner)
                        .is_some_and(|rest| rest.starts_with('/')) =>
            {
                Ok(named.to_owned())
            }
            (None, Some(named)) => Err(format!(
                "\"{named}\" is no repository of the owner {owner}; give one as {owner}/name"
            )),
        }
    }

    /// What a merge queue asking `asked` of the source on `forge` is given,
    /// or why it takes none so.
    ///
    /// # Errors
    ///
    /// Where the repository is not the source's, the method or `retries`
    /// is none the forge offers, or the write credential is an App off
    /// GitHub.
    pub fn merging(&self, forge: Forge, asked: &MergeAsked<'_>) -> Result<Merging, Unmerged> {
        let refused = |key, why: &str| Unmerged {
            key,
            why: why.to_owned(),
        };
        let repository = self
            .merged_in(forge, asked.repository)
            .map_err(|why| refused("repository", &why))?;
        if forge == Forge::GitHub && asked.method == Some(Method::FastForward) {
            return Err(refused("method", GITHUB_FAST_FORWARD));
        }
        if forge == Forge::Forgejo && asked.retries {
            return Err(refused("retries", FORGEJO_RETRIES));
        }
        if forge != Forge::GitHub && asked.write.is_some_and(|write| write.app.is_some()) {
            return Err(refused("write", GITHUB_APPS));
        }
        Ok(Merging {
            forge: forge.kind(),
            repository,
            api: api_of(forge, self),
            update: self.updating(forge),
            queueless: (forge == Forge::Forgejo).then_some(FORGEJO_QUEUELESS),
            reads: crate::merge_read::scopes(forge),
        })
    }
}

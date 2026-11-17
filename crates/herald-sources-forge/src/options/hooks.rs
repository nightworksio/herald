//! How a GitLab owner's members come to send herald their webhooks, where
//! the group cannot (ADR-0041).

use herald_kinds::Registering;

use super::SourceOptions;
use crate::Forge;

/// Who registers a project webhook on each member of a GitLab owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemberHooks {
    /// No one: the group's own webhook where its tier has them, and polls.
    #[default]
    Off,
    /// `herald hooks <name>`, run by the operator.
    Setup,
    /// `herald hooks <name>`, and each poll round for a member that
    /// has none, which needs a token with the `api` scope that may maintain
    /// each project.
    Round,
}

impl SourceOptions {
    /// Who registers a project webhook on each member.
    #[must_use]
    pub fn member_hooks(&self) -> MemberHooks {
        self.member_hooks.unwrap_or_default()
    }

    /// Who registers a webhook on each member, as the flows read it.
    #[must_use]
    pub fn registering(&self) -> Registering {
        match self.member_hooks() {
            MemberHooks::Off => Registering::Never,
            MemberHooks::Setup => Registering::Setup,
            MemberHooks::Round => Registering::Round,
        }
    }

    /// Whether `member_hooks` is sound on `forge`: a GitLab owner's, beside
    /// the `webhook` each hook is signed with.
    pub(super) fn hooks_check(&self, forge: Forge) -> Result<(), String> {
        match self.member_hooks() {
            MemberHooks::Off => Ok(()),
            _ if forge != Forge::GitLab => {
                Err("member_hooks is a GitLab owner's; remove it".to_owned())
            }
            _ if self.webhook.is_none() => Err(
                "member_hooks registers project hooks signed with webhook; give webhook, or remove member_hooks".to_owned(),
            ),
            MemberHooks::Setup | MemberHooks::Round => Ok(()),
        }
    }
}

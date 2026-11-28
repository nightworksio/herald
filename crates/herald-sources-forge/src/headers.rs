//! The headers a forge's webhook delivery names its event in.

use crate::forge::Forge;

/// The header GitHub names a delivery's event in.
const GITHUB_EVENT: &str = "x-github-event";
/// The header GitLab names a delivery's event in.
const GITLAB_EVENT: &str = "x-gitlab-event";
/// The header Forgejo names a delivery's event in.
const FORGEJO_EVENT: &str = "x-forgejo-event";
/// The header Gitea names a delivery's event in.
const GITEA_EVENT: &str = "x-gitea-event";

impl Forge {
    /// The header a delivery of the forge names its event in.
    #[must_use]
    pub const fn event_header(self) -> &'static str {
        match self {
            Self::GitHub => GITHUB_EVENT,
            Self::GitLab => GITLAB_EVENT,
            Self::Forgejo => FORGEJO_EVENT,
        }
    }

    /// The headers herald reads a delivery of the forge's event name from:
    /// its own, and on Forgejo also Gitea's.
    #[must_use]
    pub const fn event_headers(self) -> &'static [&'static str] {
        match self {
            Self::GitHub => &[GITHUB_EVENT],
            Self::GitLab => &[GITLAB_EVENT],
            Self::Forgejo => &[FORGEJO_EVENT, GITEA_EVENT],
        }
    }

    /// The event name the forge's delivery of a release carries.
    #[must_use]
    pub const fn release_event(self) -> &'static str {
        match self {
            Self::GitHub | Self::Forgejo => "release",
            Self::GitLab => "Release Hook",
        }
    }

    /// The event name `headers` give a delivery of the forge, where they
    /// give one.
    #[must_use]
    pub fn event_name<'h>(self, headers: &[(&str, &'h str)]) -> Option<&'h str> {
        let names = self.event_headers();
        headers
            .iter()
            .find(|(name, _)| names.iter().any(|header| name.eq_ignore_ascii_case(header)))
            .map(|(_, value)| *value)
    }
}

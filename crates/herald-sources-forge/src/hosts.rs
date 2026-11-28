//! Which forge serves a web host, and where its REST API is.

use crate::forge::Forge;

/// The suffix of a GHE.com tenant's web host, such as `acme.ghe.com`.
const GHE_COM: &str = ".ghe.com";

impl Forge {
    /// The web host of the forge's public instance: github.com, gitlab.com
    /// and Codeberg.
    #[must_use]
    pub const fn public_host(self) -> &'static str {
        match self {
            Self::GitHub => "github.com",
            Self::GitLab => "gitlab.com",
            Self::Forgejo => "codeberg.org",
        }
    }

    /// The forge whose web pages `host` serves, where herald knows it
    /// unasked: a public instance's, or a GHE.com tenant's.
    #[must_use]
    pub fn at(host: &str) -> Option<Self> {
        if host.ends_with(GHE_COM) {
            return Some(Self::GitHub);
        }
        Self::ALL
            .into_iter()
            .find(|forge| forge.public_host() == host)
    }

    /// The REST API of the forge's instance whose web pages `host` serves:
    /// the public instance's own; a GHE.com tenant's `https://api.<host>`;
    /// a GitHub Enterprise Server's `https://<host>/api/v3`, a GitLab's
    /// `/api/v4` and a Forgejo's `/api/v1`.
    #[must_use]
    pub fn api_at(self, host: &str) -> String {
        if host == self.public_host() {
            return self.api().url.to_owned();
        }
        match self {
            Self::GitHub if host.ends_with(GHE_COM) => format!("https://api.{host}"),
            Self::GitHub => format!("https://{host}/api/v3"),
            Self::GitLab => format!("https://{host}/api/v4"),
            Self::Forgejo => format!("https://{host}/api/v1"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_instance_s_host_names_its_forge_and_its_own_api() {
        for (forge, api) in [
            (Forge::GitHub, "https://api.github.com"),
            (Forge::GitLab, "https://gitlab.com/api/v4"),
            (Forge::Forgejo, "https://codeberg.org/api/v1"),
        ] {
            assert_eq!(Forge::at(forge.public_host()), Some(forge));
            assert_eq!(forge.api_at(forge.public_host()), api);
        }
    }

    #[test]
    fn a_ghe_com_tenant_is_github_with_its_api_host() {
        assert_eq!(Forge::at("acme.ghe.com"), Some(Forge::GitHub));
        assert_eq!(
            Forge::GitHub.api_at("acme.ghe.com"),
            "https://api.acme.ghe.com"
        );
    }

    #[test]
    fn another_host_is_asked_and_takes_its_forge_s_api_path() {
        assert_eq!(Forge::at("forge.example"), None);
        assert_eq!(Forge::at("ghe.com"), None);
        assert_eq!(
            Forge::GitHub.api_at("ghe.example"),
            "https://ghe.example/api/v3"
        );
        assert_eq!(
            Forge::GitLab.api_at("gitlab.example"),
            "https://gitlab.example/api/v4"
        );
        assert_eq!(
            Forge::Forgejo.api_at("forge.example"),
            "https://forge.example/api/v1"
        );
    }
}

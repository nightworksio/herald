use crate::forge::Api;

/// Codeberg's, the Forgejo most projects are on.
pub(crate) const API: Api = Api {
    url: "https://codeberg.org/api/v1",
    page: "limit",
    tagged: "/releases/tags/",
    event: "Forgejo's release event",
    repository: "Forgejo's repository",
    releases: "Forgejo's releases",
    release: "Forgejo's release",
};

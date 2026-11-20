use std::collections::BTreeMap;

use herald_core::{Duration, SHA256_HEX_DIGITS, TokenAlgorithm};

use crate::oidc::{Replay, Taking};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use toml::Spanned;

/// An app of the HTTP API (ADR-0007), which may also push into a herald
/// source as the peers it stands for.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "App")]
#[schemars(extend("anyOf" = [
    { "required": ["projects"], "properties": { "projects": { "minItems": 1 } } },
    { "required": ["peers"], "properties": { "peers": { "minItems": 1 } } },
]))]
pub(crate) struct AppFile {
    /// The SHA-256 of each of the app's tokens, written `sha256:` and 64
    /// lowercase hex digits, as `herald token new` prints it. A token is
    /// rotated by adding the new one before the old one is removed.
    #[serde(default)]
    #[schemars(with = "Vec<TokenHash>")]
    pub(crate) tokens: Vec<Spanned<String>>,
    /// The ID tokens it takes as credentials: each the issuer of a CI run's
    /// ID token and what such a token must say, so a run of GitHub Actions
    /// or GitLab CI proves who it is and keeps no stored secret.
    #[serde(default)]
    pub(crate) oidc: Vec<OidcFile>,
    /// The projects it may announce, each a glob ignoring case, such as
    /// `"plugin-*"`; at least one, unless it stands for `peers` alone. A
    /// project named with no wildcard is one the app announces as; a glob
    /// only scopes what it may announce, and takes no project from a
    /// source or member.
    #[serde(default)]
    #[schemars(example = AppFile::PROJECTS)]
    pub(crate) projects: Vec<String>,
    /// The routes it announces to, each naming the app among its sources;
    /// every such route where it names none.
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub(crate) routes: Vec<Spanned<String>>,
    /// The names it stands for when a run holding one of its ID tokens
    /// pushes into a herald source, such as a repository's herald running
    /// in CI, which has no URL or keys of its own. A herald source grants
    /// each name in its `peers`, as it grants a peer; no name is a table of
    /// `[peers]` or another app's.
    #[serde(default)]
    #[schemars(example = AppFile::PEERS)]
    pub(crate) peers: Vec<String>,
    /// How often one of its ID tokens may be presented: `"once"`, where
    /// the store keeps deliveries, remembers each token taken until it
    /// expires and refuses it a second time, so a captured token is
    /// worth nothing once used; `"until-expiry"` takes it as often as it
    /// comes until it expires. `"once"` where absent and the store keeps
    /// deliveries, else `"until-expiry"`, which `herald doctor` names.
    pub(crate) replay: Option<Replay>,
}

impl AppFile {
    /// How a token's SHA-256 is written in `tokens`, before its hex digits.
    pub(crate) const TOKEN_PREFIX: &str = "sha256:";
    /// The projects the schema and the loader's errors show as an example.
    pub(crate) const PROJECTS: [&str; 1] = ["herald"];
    /// The names the schema shows an app standing for.
    const PEERS: [&str; 1] = ["plugin-ci"];

    /// The SHA-256 `token` names, where it is written `sha256:` and 64
    /// lowercase hex digits.
    pub(crate) fn hash(token: &str) -> Option<&str> {
        token.strip_prefix(Self::TOKEN_PREFIX).filter(|hash| {
            hash.len() == SHA256_HEX_DIGITS
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }
}

/// The issuer of ID tokens an app takes, and what each token must say.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Oidc")]
pub(crate) struct OidcFile {
    /// The issuer, `https://`, exactly as its tokens' `iss` claim names it:
    /// `https://token.actions.githubusercontent.com` for GitHub Actions,
    /// with `/<enterprise>` after it where an enterprise of GitHub
    /// Enterprise Cloud issues its own; `https://token.actions.<enterprise>.ghe.com`
    /// on GHE.com; `https://<host>/_services/token` on GitHub Enterprise
    /// Server; `https://gitlab.com`, or the instance's URL on GitLab
    /// self-managed.
    #[schemars(example = &"https://token.actions.githubusercontent.com")]
    pub(crate) issuer: String,
    /// What a token's `aud` claim must name: this herald's `[server] url`
    /// where absent, else `https://` and its `[federation] domain`. Never
    /// an issuer, a peer's URL, nor a GitHub's default `<GitHub>/<owner>`,
    /// which every herald of the owner would take alike: github.com's, or
    /// the GHE.com enterprise's or GitHub Enterprise Server's the issuer
    /// is.
    pub(crate) audience: Option<String>,
    /// Each claim a token must carry, by name, and the pattern its value
    /// must match, case and all: `*` stands for any run of characters and
    /// `?` for any one, neither reaching past a `/`. At least one binds
    /// who the run is: `repository_id`, `repository_owner_id`, `project_id`
    /// or `namespace_id` with no wildcard, `repository`,
    /// `repository_owner`, `project_path` or `namespace_path` with none
    /// before its first `/`, or `sub` as `repo:<owner>/...` or
    /// `project_path:<group>/...`, the owner whole; else a token of any
    /// repository at the issuer would be taken, as a GitHub repository
    /// shapes its own `sub`. Prefer an id a renamed or re-created
    /// repository never inherits; `herald doctor` names an entry that
    /// binds names alone, `sub` among them.
    #[serde(default)]
    #[schemars(example = OidcFile::claims_example())]
    pub(crate) claims: BTreeMap<String, String>,
    /// The algorithms a token may be signed with; `["RS256"]` where absent.
    /// What its header names is never taken on its own.
    pub(crate) algorithms: Option<Vec<TokenAlgorithm>>,
    /// How long the issuer's keys, once read, are used before they are
    /// read again: "1h" where absent, and at most "1d". Keys that cannot be
    /// read again are not used past it.
    pub(crate) keys_for: Option<Duration>,
    /// How long after the issuer's keys were last fetched, or tried, a
    /// token naming a key they hold none of has them fetched again: "1m"
    /// where absent, from "10s" to `keys_for`. A rotated key is taken
    /// within it, and however many tokens name unknown keys, the issuer is
    /// asked at most once in it.
    pub(crate) keys_retry: Option<Duration>,
    /// Where the issuer's keys are read, `https://`, in place of the
    /// `jwks_uri` its discovery document names on its own origin.
    pub(crate) jwks: Option<String>,
    /// The issuer's keys, as a JSON Web Key Set's `keys` lists them,
    /// pinned so they are never read from the network; each rotation is
    /// then the operator's to write.
    #[schemars(with = "Option<Vec<BTreeMap<String, String>>>")]
    pub(crate) keys: Option<Vec<Value>>,
    /// The longest a token may live, from its `iat` to its `exp`: "1h"
    /// where absent, at most "1d". A longer-lived token is refused, so a
    /// captured one is worth nothing past it; GitLab CI's tokens live as
    /// long as their job's timeout.
    pub(crate) lifetime: Option<Duration>,
    /// How far a token's `exp`, `nbf` and `iat` may stand from this
    /// herald's clock: "1m" where absent, at most "5m".
    pub(crate) skew: Option<Duration>,
    /// What its tokens may do: `"announce"` through the HTTP API, `"push"`
    /// into a herald source as a name the app stands for, or both. Where
    /// absent, what the app does; an app naming both projects and peers
    /// says it on each entry, so no token is taken by both unless its entry
    /// grants both.
    pub(crate) takes: Option<Vec<Taking>>,
}

impl OidcFile {
    /// The claims the schema shows an entry binding.
    fn claims_example() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("repository_id".to_owned(), "123456".to_owned()),
            ("ref".to_owned(), "refs/tags/v*".to_owned()),
        ])
    }
}

/// A token's SHA-256, as `[apps.<name>] tokens` writes it.
struct TokenHash;

impl JsonSchema for TokenHash {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "TokenHash".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": format!("^{}[0-9a-f]{{{}}}$", AppFile::TOKEN_PREFIX, SHA256_HEX_DIGITS)
        })
    }
}

//! `[federation]` as written: this herald among other instances, and
//! where it writes its static signed log.

use herald_core::follows::{Approve, FollowerCaps, Mode, SharedList};
use herald_core::{Duration, Rate, SecretRef, Size};
use schemars::JsonSchema;
use serde::Deserialize;

/// This herald among other instances: the domain it serves, its keys, and
/// what its ActivityPub actors take where their destination leaves it out.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Federation")]
pub(crate) struct FederationFile {
    /// The domain herald serves its discovery document and actors on, such
    /// as `herald.example`: a host alone, with no scheme, port or path. An
    /// ActivityPub destination that names no `domain` takes it.
    #[schemars(example = "herald.example")]
    pub(crate) domain: Option<String>,
    /// The secrets holding the instance's Ed25519 private keys, each as
    /// PKCS#8 PEM, such as `openssl genpkey -algorithm ed25519` writes,
    /// newest first: the first signs, and every one verifies. Rotate by
    /// writing a new key before the old one, and leave the old one out once
    /// peers know the new one.
    #[serde(default)]
    pub(crate) keys: Vec<SecretRef>,
    /// How long a peer may keep the instance's key set before it reads it
    /// again; "1d" where absent, at most "7d".
    pub(crate) refresh: Option<Duration>,
    /// How long each response of herald's API, and each push it makes to a
    /// peer, stays signed, from when it is made: "5m" where absent, at most
    /// "20m".
    pub(crate) signed_for: Option<Duration>,
    /// The most bytes a peer's signature field, such as `Signature` or
    /// `Content-Digest`, may hold and still be read: "8KiB" where absent,
    /// at most "32KiB". A longer one is refused before it is parsed.
    pub(crate) fields: Option<Size>,
    /// How long what each peer asked is kept, by UTC day: "90d" where
    /// absent, at most "365d" (ADR-0054).
    pub(crate) keep_audit: Option<Duration>,
    /// How soon before a grant ends doctor names it: "7d" where absent, at
    /// most "90d" (ADR-0054).
    pub(crate) grants_ending: Option<Duration>,
    /// How many requests to a source's inbox that no peer's key signs are
    /// kept in the audit: "120/m" where absent, at most "480/m"; the rest
    /// are only logged (ADR-0054).
    pub(crate) stranger_rate: Option<Rate>,
    /// How many requests replaying a nonce one peer already used are kept
    /// in its audit, counted apart from its own rate so a replay spends
    /// none of it: "120/m" where absent, at most "480/m"; past it herald
    /// answers 429 (ADR-0054).
    pub(crate) replay_rate: Option<Rate>,
    /// How many requests of one peer refused past its rate have their
    /// nonces kept, counted afresh every 20 minutes, the longest a request
    /// lives, so none is taken later: 2400 where absent, its rate's default
    /// over that time, at most 9600. Past it the peer is held, and nothing
    /// of it is taken until every request it made unkept has expired
    /// (ADR-0054).
    pub(crate) refused_nonces: Option<u32>,
    /// Peer keys that are never trusted again, each a Multikey such as
    /// `z6Mk…`, as `herald peer revoke` adds them: a peer pinning one does
    /// not load, and a new key set naming one is refused.
    #[serde(default)]
    pub(crate) revoked: Vec<String>,
    /// Where herald writes its feeds as a static signed log for any static
    /// host to serve, such as GitHub Pages, at the end of each run that
    /// changed one (ADR-0060). None where absent.
    pub(crate) pages: Option<PagesFile>,
    /// The secret holding the RSA private key herald's ActivityPub actors
    /// sign with, as PKCS#8 PEM. An ActivityPub destination that names no
    /// `key` takes it.
    #[allow(
        dead_code,
        reason = "read by each ActivityPub destination from the table as written"
    )]
    pub(crate) actor_key: Option<SecretRef>,
    /// Whom herald's actors federate with, for an ActivityPub destination
    /// that names no `mode`: `open` or `allowlist`.
    #[allow(
        dead_code,
        reason = "read by each ActivityPub destination from the table as written"
    )]
    pub(crate) mode: Option<Mode>,
    /// The servers and actors herald's actors never take a follow from or
    /// deliver to, for an ActivityPub destination that names no `block`.
    pub(crate) block: Option<Vec<String>>,
    /// The servers and actors an `allowlist` federates with, for an
    /// ActivityPub destination that names no `allow`.
    pub(crate) allow: Option<Vec<String>>,
    /// How many may follow each actor, for an ActivityPub destination that
    /// names no `followers`.
    pub(crate) followers: Option<FollowerCaps>,
    /// Who takes a follow, for an ActivityPub destination that names no
    /// `approve`: `auto` or `manual`.
    #[allow(
        dead_code,
        reason = "read by each ActivityPub destination from the table as written"
    )]
    pub(crate) approve: Option<Approve>,
    /// Shared block lists, for an ActivityPub destination that names no
    /// `lists`.
    #[allow(
        dead_code,
        reason = "read by each ActivityPub destination from the table as written"
    )]
    pub(crate) lists: Option<Vec<SharedList>>,
}

/// `[federation.pages]`: where herald writes its feeds as a static signed
/// log (ADR-0060).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Pages")]
pub(crate) struct PagesFile {
    /// Where a static host serves the log, such as
    /// `https://owner.github.io/repository`: https://, `[federation]
    /// domain` as its host, and a path where the log is not served at the
    /// root.
    #[schemars(example = "https://herald.example/log")]
    pub(crate) url: String,
    /// The branch each writing of the log is committed to, one commit
    /// each, for GitHub Pages or any static host to serve; `herald-pages`
    /// where absent, unless `artifact` is named, which then takes its
    /// place.
    #[schemars(example = "herald-pages")]
    pub(crate) branch: Option<String>,
    /// The repository holding the branch, as `owner/name`; the run's own
    /// where absent.
    pub(crate) repository: Option<String>,
    /// The secret holding a token that may push to the branch;
    /// `GITHUB_TOKEN` where absent.
    pub(crate) token: Option<SecretRef>,
    /// The API's base URL, for a GitHub Enterprise Server; GitHub's own API
    /// where absent.
    pub(crate) api: Option<String>,
    /// A directory each writing of the log fills with every file of it,
    /// such as for actions/upload-pages-artifact to deploy. None where
    /// absent.
    #[schemars(example = &"_site")]
    pub(crate) artifact: Option<String>,
    /// How long a log no feed changed is left before it is written again
    /// under its next sequence, so its readers see it is current: "1d"
    /// where absent, at most "7d".
    pub(crate) refresh: Option<Duration>,
}

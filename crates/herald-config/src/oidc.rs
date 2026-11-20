//! The ID tokens an app takes as credentials (ADR-0064): each the issuer
//! of a CI run's ID token, read exactly, and what the token must say.

use std::collections::{BTreeMap, BTreeSet};

use herald_core::{ClaimPattern, KeySet, TokenAlgorithm};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;

/// How often a token an app takes may be presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Replay {
    /// Once: each token taken is remembered until it expires, and taken
    /// no second time.
    Once,
    /// As often as it is presented until it expires.
    UntilExpiry,
}

/// What a token an entry takes may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Taking {
    /// Announce through the HTTP API, as the app.
    Announce,
    /// Push into a herald source, as a name the app stands for.
    Push,
}

/// An issuer of ID tokens an app takes, and what each token must say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oidc {
    /// The issuer, exactly as its tokens' `iss` claim names it.
    pub issuer: String,
    /// What a token's `aud` claim must name.
    pub audience: String,
    /// Each claim a token must carry, and the pattern its value matches.
    pub claims: BTreeMap<String, ClaimPattern>,
    /// The algorithms a token may be signed with.
    pub algorithms: Vec<TokenAlgorithm>,
    /// How long the issuer's keys are used before they are read again.
    pub keys_for: SignedDuration,
    /// How long after the issuer's keys were last fetched, or tried, a
    /// token naming a key they lack has them fetched again.
    pub keys_retry: SignedDuration,
    /// Where the issuer's keys are read in place of the address its
    /// discovery document names.
    pub jwks: Option<String>,
    /// The issuer's keys, pinned, where they are never read.
    pub keys: Option<KeySet>,
    /// What its tokens may do.
    pub takes: BTreeSet<Taking>,
    /// The longest a token may live, from its `iat` to its `exp`.
    pub lifetime: SignedDuration,
    /// How far a token's times may stand from this herald's clock.
    pub skew: SignedDuration,
}

impl Oidc {
    /// How long an issuer's keys are used where `keys_for` is unset.
    pub const KEYS_FOR: SignedDuration = SignedDuration::from_hours(1);
    /// The longest `keys_for` allows.
    pub const KEYS_FOR_ALLOWED: SignedDuration = SignedDuration::from_hours(24);
    /// How long after a fetch the keys may be fetched again for a key they
    /// lack, where `keys_retry` is unset.
    pub const KEYS_RETRY: SignedDuration = SignedDuration::from_mins(1);
    /// The shortest `keys_retry` allows.
    pub const KEYS_RETRY_LEAST: SignedDuration = SignedDuration::from_secs(10);
    /// How long a token may live where `lifetime` is unset.
    pub const LIFETIME: SignedDuration = SignedDuration::from_hours(1);
    /// The longest `lifetime` allows.
    pub const LIFETIME_ALLOWED: SignedDuration = SignedDuration::from_hours(24);
    /// How far a token's times may stand from the clock where `skew` is
    /// unset.
    pub const SKEW: SignedDuration = SignedDuration::from_mins(1);
    /// The most `skew` allows.
    pub const SKEW_ALLOWED: SignedDuration = SignedDuration::from_mins(5);
    /// The claims that name a repository, project, owner or group by an id
    /// its forge never gives another, so a renamed or re-created one never
    /// inherits what an entry binds.
    pub const IMMUTABLE: [&str; 4] = [
        "repository_id",
        "repository_owner_id",
        "project_id",
        "namespace_id",
    ];
    /// The claims that name a repository, project, owner or group by name,
    /// its owner before the first `/`.
    pub const NAMED: [&str; 4] = [
        "repository",
        "repository_owner",
        "project_path",
        "namespace_path",
    ];
    /// The claim naming a run's subject, which a repository may shape for
    /// itself on GitHub.
    pub const SUBJECT: &str = "sub";
    /// How a subject that names its repository, owner first, begins: as
    /// GitHub's default shape and GitLab's begin it.
    pub const SUBJECTS: [&str; 2] = ["repo:", "project_path:"];

    /// Whether it binds a claim of [`Self::IMMUTABLE`] to one value.
    #[must_use]
    pub fn binds_an_id(&self) -> bool {
        ids(&self.claims)
    }

    /// The addresses its issuer's keys are read from: the issuer's, whose
    /// discovery document names them, and `jwks` where it is set; none
    /// where they are pinned.
    #[must_use]
    pub fn bases(&self) -> Vec<String> {
        if self.keys.is_some() {
            return Vec::new();
        }
        std::iter::once(self.issuer.clone())
            .chain(self.jwks.clone())
            .collect()
    }
}

/// Whether `claims` binds a claim of [`Oidc::IMMUTABLE`] to one value.
fn ids(claims: &BTreeMap<String, ClaimPattern>) -> bool {
    Oidc::IMMUTABLE
        .iter()
        .any(|claim| claims.get(*claim).is_some_and(ClaimPattern::literal))
}

/// Whether `claims` binds a claim of [`Oidc::IMMUTABLE`] to one value, a
/// claim of [`Oidc::NAMED`] to one owner, or the subject to a repository
/// of one owner: `repo:<owner>/` or `project_path:<group>/` and then
/// anything, the owner written with no wildcard. A subject of any other
/// shape is one a repository may give itself.
pub(crate) fn identified(claims: &BTreeMap<String, ClaimPattern>) -> bool {
    let subject = claims.get(Oidc::SUBJECT).is_some_and(|subject| {
        Oidc::SUBJECTS.iter().any(|shape| {
            subject
                .as_str()
                .strip_prefix(shape)
                .is_some_and(|owned| owned.contains('/') && ClaimPattern::new(owned).owned())
        })
    });
    ids(claims)
        || subject
        || Oidc::NAMED
            .iter()
            .any(|claim| claims.get(*claim).is_some_and(ClaimPattern::owned))
}

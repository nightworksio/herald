use herald_core::{SecretRef, Symbols, WaitOptions};
use schemars::JsonSchema;
use serde::Deserialize;

use super::policy::{Approve, FollowerCaps, Mode, Policy, unlisted};

/// The longest name an actor takes, as Mastodon takes a username.
const NAME: usize = 30;

/// Where herald's own actors live, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActorOptions {
    /// The domain herald serves its actors on, such as `herald.example`:
    /// a host alone, with no scheme, port or path.
    pub domain: String,
    /// The secret holding the actors' RSA private key, as PKCS#8 PEM, such
    /// as `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048`
    /// writes.
    pub key: SecretRef,
    /// The characters its posts write where its markup has none: a task
    /// done and to do, a rule and a text list's bullet.
    pub symbols: Option<Symbols>,
    /// How long it waits before sending again where an inbox answers that it
    /// is unwell or limited and names no wait.
    pub waits: Option<WaitOptions>,
    /// How many Notes one page of each actor's outbox holds, from 1 to 80;
    /// 20 where unset.
    pub outbox_page: Option<usize>,
    /// Whom its actors federate with: `open`, everyone `block` does not
    /// name, or `allowlist`, only those `allow` names.
    #[serde(default)]
    pub mode: Mode,
    /// The servers and actors its actors never take a follow from or
    /// deliver to: each a domain, naming its subdomains too, or an actor's
    /// id.
    #[serde(default)]
    pub block: Vec<String>,
    /// The servers and actors an `allowlist` federates with, written as
    /// `block` writes them.
    #[serde(default)]
    pub allow: Vec<String>,
    /// How many may follow each actor, in all and from any one server; a
    /// follow past either is answered with a `Reject`.
    pub followers: Option<FollowerCaps>,
    /// Who takes a follow: `auto`, herald by the lists and caps, or
    /// `manual`, the operator through `herald followers`.
    #[serde(default)]
    pub approve: Approve,
}

impl ActorOptions {
    /// The Notes an outbox page holds where the config sets none.
    pub const OUTBOX_PAGE: usize = 20;
    /// The most Notes an outbox page may hold: four times the default.
    pub const OUTBOX_MOST: usize = 4 * Self::OUTBOX_PAGE;

    /// Whom its actors federate with, and how many may follow each.
    #[must_use]
    pub fn policy(&self) -> Policy {
        Policy::new(
            self.mode,
            &self.block,
            &self.allow,
            self.followers.unwrap_or_default(),
        )
        .approving(self.approve)
    }

    /// How many Notes one page of each actor's outbox holds.
    #[must_use]
    pub fn outbox_page(&self) -> usize {
        self.outbox_page.unwrap_or(Self::OUTBOX_PAGE)
    }

    /// Whether the domain is a host alone, every entry of `block` and
    /// `allow` is a domain or an actor's id, each follower cap is at least
    /// 1, and an outbox page holds 1 to 80 Notes.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if !herald_core::domain(&self.domain) {
            return Err(format!(
                "\"{}\" is not a domain; give the host herald serves its actors on, such as \"herald.example\", with no scheme, port or path",
                self.domain
            ));
        }
        if let Some(refused) = self
            .block
            .iter()
            .chain(&self.allow)
            .find_map(|entry| unlisted(entry))
        {
            return Err(refused);
        }
        let caps = self.followers.unwrap_or_default();
        if caps.most == Some(0) || caps.per_domain == Some(0) {
            return Err("followers.most and followers.per_domain take at least 1; leave one out for no limit".to_owned());
        }
        if !(1..=Self::OUTBOX_MOST).contains(&self.outbox_page()) {
            return Err(format!(
                "outbox_page holds 1 to {} Notes, not {}",
                Self::OUTBOX_MOST,
                self.outbox_page()
            ));
        }
        Ok(())
    }
}

/// What a route says about herald's actor alone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActorRouteOptions {
    /// The actor the route announces as, such as `php-mutation-gate`, which
    /// is followed as `@php-mutation-gate@<domain>`.
    pub actor: String,
    /// The content warning each Note is shown behind.
    pub content_warning: Option<String>,
    /// Who sees each Note on the timelines.
    #[serde(default)]
    pub visibility: Audience,
    /// The language each Note is written in, as an ISO 639 code.
    pub language: Option<String>,
}

/// Who sees an actor's Notes on the timelines. Followers see every Note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Audience {
    /// Everyone, on the public timelines too.
    #[default]
    Public,
    /// Everyone who looks, kept off the public timelines.
    Unlisted,
}

impl ActorRouteOptions {
    /// Whether the actor's name is one a follower can write.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if let Some(language) = self
            .language
            .as_deref()
            .filter(|code| !crate::options::language_code(code))
        {
            return Err(crate::options::unwritten_language(language));
        }
        let sound = !self.actor.is_empty()
            && self.actor.len() <= NAME
            && !self.actor.starts_with('-')
            && !self.actor.ends_with('-')
            && self.actor.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
            });
        if sound {
            Ok(())
        } else {
            Err(format!(
                "\"{}\" is not an actor's name; give at most {NAME} lowercase letters, digits, `_` or a `-` within, such as \"php-mutation-gate\"",
                self.actor
            ))
        }
    }
}

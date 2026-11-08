use herald_core::{SecretRef, Symbols, WaitOptions};
use schemars::JsonSchema;
use serde::Deserialize;

/// The longest name an actor takes, as Mastodon takes a username.
const NAME: usize = 30;

/// Where herald's own actors live, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActorOptions {
    /// The domain herald serves its actors on, such as `herald.example`:
    /// a host alone, with no scheme, port or path.
    pub domain: String,
    /// The secret holding the actors' RSA private key, as PKCS#8 PEM, which
    /// `herald activitypub keygen` writes.
    pub key: SecretRef,
    /// The characters its posts write where its markup has none: a task
    /// done and to do, a rule and a text list's bullet.
    pub symbols: Option<Symbols>,
    /// How long it waits before sending again where an inbox answers that it
    /// is unwell or limited and names no wait.
    pub waits: Option<WaitOptions>,
}

impl ActorOptions {
    /// Whether the domain is a host alone.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if herald_core::domain(&self.domain) {
            Ok(())
        } else {
            Err(format!(
                "\"{}\" is not a domain; give the host herald serves its actors on, such as \"herald.example\", with no scheme, port or path",
                self.domain
            ))
        }
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

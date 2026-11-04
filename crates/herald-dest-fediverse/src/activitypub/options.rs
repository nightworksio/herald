use herald_core::SecretRef;
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
}

impl ActorOptions {
    /// Whether the domain is a host alone.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        let labels: Vec<&str> = self.domain.split('.').collect();
        let sound = labels.len() > 1
            && labels.iter().all(|label| {
                !label.is_empty()
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            });
        if sound {
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
}

impl ActorRouteOptions {
    /// Whether the actor's name is one a follower can write.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
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

use std::fmt;
use std::future::Future;

/// A secret's value, with the name the config gives it. Its `Debug` prints the
/// name alone, and it has no `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    name: String,
    value: String,
}

impl Secret {
    /// The secret `name`, whose value is `value`.
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// The name the config gives it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The value, for the adapter that sends it and nothing else.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.value
    }

    /// How the secret is shown wherever a value would be: `«secret:NAME»`.
    #[must_use]
    pub fn redacted(&self) -> String {
        format!("«secret:{}»", self.name)
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

/// One piece of a [`Text`]: plain, or a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Plain text.
    Plain(String),
    /// A secret, redacted wherever the text is shown.
    Secret(Secret),
}

/// A text that may carry secrets: a URL or a header value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Text(Vec<Segment>);

impl Text {
    /// The plain text `text`.
    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        Self(vec![Segment::Plain(text.into())])
    }

    /// The secret `secret`, alone.
    #[must_use]
    pub fn secret(secret: Secret) -> Self {
        Self(vec![Segment::Secret(secret)])
    }

    /// The text followed by the plain `text`.
    #[must_use]
    pub fn then(mut self, text: impl Into<String>) -> Self {
        self.0.push(Segment::Plain(text.into()));
        self
    }

    /// The text followed by `secret`.
    #[must_use]
    pub fn then_secret(mut self, secret: Secret) -> Self {
        self.0.push(Segment::Secret(secret));
        self
    }

    /// The text with every secret's value, for the adapter that sends it.
    #[must_use]
    pub fn expose(&self) -> String {
        self.0
            .iter()
            .map(|segment| match segment {
                Segment::Plain(text) => text.as_str(),
                Segment::Secret(secret) => secret.expose(),
            })
            .collect()
    }

    /// The text with every secret redacted.
    #[must_use]
    pub fn redacted(&self) -> String {
        self.0
            .iter()
            .map(|segment| match segment {
                Segment::Plain(text) => text.clone(),
                Segment::Secret(secret) => secret.redacted(),
            })
            .collect()
    }

    /// The pieces of the text.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }
}

/// Why a secret could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretError {
    /// No secret has the name.
    Missing {
        /// The name asked for.
        name: String,
    },
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { name } => write!(
                f,
                "no secret is named {name}; set it where this platform keeps secrets"
            ),
        }
    }
}

/// Where secrets are read: the environment on a runner, a platform's secrets
/// on a serverless platform.
pub trait Secrets: Sync {
    /// The secret named `name`.
    fn secret(&self, name: &str) -> impl Future<Output = Result<Secret, SecretError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> Secret {
        Secret::new("HERALD_DISCORD_TOKEN", "hunter2")
    }

    #[test]
    fn a_secret_shows_its_name_and_never_its_value() {
        let secret = token();
        assert_eq!(secret.name(), "HERALD_DISCORD_TOKEN");
        assert_eq!(secret.expose(), "hunter2");
        assert_eq!(format!("{secret:?}"), "«secret:HERALD_DISCORD_TOKEN»");
        assert_eq!(secret.redacted(), "«secret:HERALD_DISCORD_TOKEN»");
    }

    #[test]
    fn a_text_exposes_its_secrets_only_when_asked() {
        let text = Text::plain("Bot ").then_secret(token()).then("!");
        assert_eq!(text.expose(), "Bot hunter2!");
        assert_eq!(text.redacted(), "Bot «secret:HERALD_DISCORD_TOKEN»!");
        assert!(!format!("{text:?}").contains("hunter2"));
        assert_eq!(text.segments().len(), 3);
    }

    #[test]
    fn a_secret_alone_is_a_text() {
        let text = Text::secret(token());
        assert_eq!(text.expose(), "hunter2");
        assert_eq!(text.redacted(), "«secret:HERALD_DISCORD_TOKEN»");
        assert_eq!(Text::default().expose(), "");
    }

    #[test]
    fn a_missing_secret_names_itself_and_the_fix() {
        let error = SecretError::Missing {
            name: "HERALD_DISCORD_TOKEN".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "no secret is named HERALD_DISCORD_TOKEN; set it where this platform keeps secrets"
        );
    }
}

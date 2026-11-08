use herald_core::{ReadBack, SecretRef};
use schemars::JsonSchema;
use serde::Deserialize;

/// How a Mastodon destination is reached, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DestinationOptions {
    /// The server's base URL, such as `https://mastodon.social`.
    pub server: String,
    /// The secret holding the account's access token, granted
    /// `write:statuses` and `read:statuses`.
    pub token: SecretRef,
    /// How many pages of its own posts it reads back for their markers,
    /// from 1 to 20; 5 where unset.
    pub read_back: Option<ReadBack>,
}

/// Who sees a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    /// Everyone, on the public timelines too.
    #[default]
    Public,
    /// Everyone, kept off the public timelines.
    Unlisted,
    /// The account's followers alone.
    Private,
}

impl Visibility {
    /// The visibility as the Mastodon API spells it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Unlisted => "unlisted",
            Self::Private => "private",
        }
    }
}

/// What a route says about Mastodon alone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {
    /// Who sees each status.
    #[serde(default)]
    pub visibility: Visibility,
    /// The language each status is written in, as an ISO 639 code.
    pub language: Option<String>,
    /// The content warning each status is shown behind.
    pub content_warning: Option<String>,
    /// The most characters the server takes in a status, Mastodon's own
    /// 500 where it is not given.
    pub limit: Option<usize>,
}

impl RouteOptions {
    /// Whether the options hold together.
    ///
    /// # Errors
    ///
    /// The first option that does not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if let Some(language) = &self.language
            && (!(2..=3).contains(&language.len())
                || !language.bytes().all(|byte| byte.is_ascii_lowercase()))
        {
            return Err(format!(
                "\"{language}\" is not an ISO 639 language code; give two or three lowercase letters, such as \"en\""
            ));
        }
        let shortest = crate::shortest(self);
        if self.limit.is_some_and(|limit| limit < shortest) {
            return Err(format!(
                "a status needs at least {shortest} characters for its title, link and content warning; raise limit to the server's own"
            ));
        }
        Ok(())
    }
}

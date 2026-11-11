use crate::secret::Secret;

/// How herald is allowed to post where a destination takes a bot or an
/// incoming webhook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// A bot's token.
    Bot(Secret),
    /// An incoming webhook's URL, which carries its own token.
    Webhook(Secret),
}

impl Credential {
    /// The bot's token, where it is a bot's.
    #[must_use]
    pub fn bot(&self) -> Option<&Secret> {
        match self {
            Self::Bot(token) => Some(token),
            Self::Webhook(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bot_has_a_token() {
        let token = Secret::new("T", "t");
        assert_eq!(Credential::Bot(token.clone()).bot(), Some(&token));
        assert_eq!(Credential::Webhook(token).bot(), None);
    }
}

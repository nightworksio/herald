//! What a source reads with: a token the config names, or one minted for it
//! (ADR-0041).

use std::sync::{Arc, PoisonError, RwLock};

use herald_ports::Secret;

/// A token minted for a source, as its last refresh left it.
#[derive(Debug, Default)]
pub struct Minted(RwLock<Option<Secret>>);

impl Minted {
    /// The token the last refresh minted, where one did.
    #[must_use]
    pub fn current(&self) -> Option<Secret> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// `token` kept as the one requests read with, or none where minting
    /// failed.
    pub fn refresh(&self, token: Option<Secret>) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = token;
    }
}

impl PartialEq for Minted {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for Minted {}

/// What a source reads with.
#[derive(Debug, Clone)]
pub enum Credential {
    /// A token the config names.
    Named(Secret),
    /// A token minted for the source, refreshed before each round.
    Minted(Arc<Minted>),
}

impl Credential {
    /// The token the config names, where it names one; a kind that mints
    /// none reads only such a token.
    #[must_use]
    pub fn named(self) -> Option<Secret> {
        match self {
            Self::Named(secret) => Some(secret),
            Self::Minted(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_token_is_read_as_its_last_refresh_left_it() {
        let minted = Arc::new(Minted::default());
        assert_eq!(minted.current(), None);
        minted.refresh(Some(Secret::new("A", "one")));
        assert_eq!(
            minted.current().map(|token| token.expose().to_owned()),
            Some("one".to_owned())
        );
        let credential = Credential::Minted(Arc::clone(&minted));
        assert_eq!(credential.named(), None);
        minted.refresh(None);
        assert_eq!(minted.current(), None);
        assert_eq!(*minted, *minted);
        assert_ne!(*minted, Minted::default());
        let named = Credential::Named(Secret::new("B", "two"));
        assert_eq!(
            named.named().map(|token| token.expose().to_owned()),
            Some("two".to_owned())
        );
    }
}

//! Minted tokens kept between runs, each sealed (ADR-0041).

use std::future::Future;

use herald_core::KeptToken;

use super::{StoreError, Version, Versioned, Written};

/// Minted tokens, each record written with compare-and-set.
pub trait Tokens: Sync {
    /// The token kept as `name`, where there is one.
    fn token(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<Versioned<KeptToken>>, StoreError>> + Send;

    /// Writes `token` if the stored one is at `expected`, or absent when
    /// `expected` is `None`.
    fn put_token(
        &self,
        token: &KeptToken,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

impl Tokens for super::Unkept {
    fn token(
        &self,
        _: &str,
    ) -> impl Future<Output = Result<Option<Versioned<KeptToken>>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Tokens.unsupported()))
    }

    fn put_token(
        &self,
        _: &KeptToken,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        std::future::ready(Err(super::Held::Tokens.unsupported()))
    }
}

impl<T: Tokens> Tokens for Option<&T> {
    async fn token(&self, name: &str) -> Result<Option<Versioned<KeptToken>>, StoreError> {
        match self {
            Some(kept) => kept.token(name).await,
            None => super::Unkept.token(name).await,
        }
    }

    async fn put_token(
        &self,
        token: &KeptToken,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_token(token, expected).await,
            None => super::Unkept.put_token(token, expected).await,
        }
    }
}

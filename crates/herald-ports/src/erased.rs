use std::future::Future;
use std::pin::Pin;

use herald_core::{Entry, Key, Marker};

use crate::crypto::{Algorithm, CryptoError, Signer};
use crate::http::{Http, HttpError, Request, Response};
use crate::markers::{MarkedThread, Markers, MarkersError};
use crate::secret::{Secret, SecretError, Secrets};

/// A future boxed, as a trait object's methods return one.
pub type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// [`Http`] as a trait object: every `Http` is one.
pub trait DynHttp: Sync {
    /// The response to `request`.
    fn send_boxed(&self, request: Request) -> Boxed<'_, Result<Response, HttpError>>;
}

impl<H: Http> DynHttp for H {
    fn send_boxed(&self, request: Request) -> Boxed<'_, Result<Response, HttpError>> {
        Box::pin(self.send(request))
    }
}

/// [`Signer`] as a trait object: every `Signer` is one.
pub trait DynSigner: Sync {
    /// The signature of `message` by `key` under `algorithm`.
    fn sign_boxed<'a>(
        &'a self,
        algorithm: Algorithm,
        key: &'a Secret,
        message: &'a [u8],
    ) -> Boxed<'a, Result<Vec<u8>, CryptoError>>;

    /// The public half of `key` under `algorithm`.
    fn public_boxed<'a>(
        &'a self,
        algorithm: Algorithm,
        key: &'a Secret,
    ) -> Boxed<'a, Result<Vec<u8>, CryptoError>>;
}

impl<G: Signer> DynSigner for G {
    fn sign_boxed<'a>(
        &'a self,
        algorithm: Algorithm,
        key: &'a Secret,
        message: &'a [u8],
    ) -> Boxed<'a, Result<Vec<u8>, CryptoError>> {
        Box::pin(self.sign(algorithm, key, message))
    }

    fn public_boxed<'a>(
        &'a self,
        algorithm: Algorithm,
        key: &'a Secret,
    ) -> Boxed<'a, Result<Vec<u8>, CryptoError>> {
        Box::pin(self.public(algorithm, key))
    }
}

/// [`Secrets`] as a trait object: every `Secrets` is one.
pub trait DynSecrets: Sync {
    /// The secret named `name`.
    fn secret_boxed<'a>(&'a self, name: &'a str) -> Boxed<'a, Result<Secret, SecretError>>;
}

impl<S: Secrets> DynSecrets for S {
    fn secret_boxed<'a>(&'a self, name: &'a str) -> Boxed<'a, Result<Secret, SecretError>> {
        Box::pin(self.secret(name))
    }
}

/// [`Markers`] as a trait object: every `Markers` is one.
pub trait DynMarkers: Sync {
    /// The ledger entry for `key` made by herald's own post carrying
    /// `marker`.
    fn entry_boxed<'a>(
        &'a self,
        key: &'a Key,
        marker: &'a Marker,
    ) -> Boxed<'a, Result<Option<Entry>, MarkersError>>;

    /// The threads opened from herald's own recent posts carrying `route`.
    fn threads_boxed<'a>(
        &'a self,
        route: &'a Marker,
    ) -> Boxed<'a, Result<Vec<MarkedThread>, MarkersError>>;
}

impl<M: Markers> DynMarkers for M {
    fn entry_boxed<'a>(
        &'a self,
        key: &'a Key,
        marker: &'a Marker,
    ) -> Boxed<'a, Result<Option<Entry>, MarkersError>> {
        Box::pin(self.entry(key, marker))
    }

    fn threads_boxed<'a>(
        &'a self,
        route: &'a Marker,
    ) -> Boxed<'a, Result<Vec<MarkedThread>, MarkersError>> {
        Box::pin(self.threads(route))
    }
}

/// A trait object's port, as code generic over the port takes it.
#[derive(Clone, Copy)]
pub struct Erased<'a, T: ?Sized>(pub &'a T);

impl Http for Erased<'_, dyn DynHttp + '_> {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        self.0.send_boxed(request)
    }
}

impl Signer for Erased<'_, dyn DynSigner + '_> {
    async fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.0.sign_boxed(algorithm, key, message).await
    }

    async fn public(&self, algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        self.0.public_boxed(algorithm, key).await
    }
}

impl Markers for Erased<'_, dyn DynMarkers + '_> {
    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        self.0.entry_boxed(key, marker).await
    }

    async fn threads(&self, route: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        self.0.threads_boxed(route).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]

    use super::*;

    struct Echo;

    impl Http for Echo {
        async fn send(&self, request: Request) -> Result<Response, HttpError> {
            Ok(Response {
                status: 200,
                headers: Vec::new(),
                body: request.url.expose().into_bytes(),
            })
        }
    }

    struct Reversed;

    impl Signer for Reversed {
        async fn sign(
            &self,
            _: Algorithm,
            key: &Secret,
            message: &[u8],
        ) -> Result<Vec<u8>, CryptoError> {
            if key.expose().is_empty() {
                return Err(CryptoError::BadKey {
                    name: key.name().to_owned(),
                });
            }
            Ok(message.iter().rev().copied().collect())
        }

        async fn public(&self, _: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
            Ok(key.expose().as_bytes().to_vec())
        }
    }

    struct Named;

    impl Secrets for Named {
        async fn secret(&self, name: &str) -> Result<Secret, SecretError> {
            if name.is_empty() {
                return Err(SecretError::Missing {
                    name: name.to_owned(),
                });
            }
            Ok(Secret::new(name, "value"))
        }
    }

    struct Found;

    impl Markers for Found {
        async fn entry(&self, key: &Key, _: &Marker) -> Result<Option<Entry>, MarkersError> {
            Ok(Some(Entry::new(*key)))
        }

        async fn threads(&self, _: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
            Ok(vec![MarkedThread {
                thread: "7".to_owned(),
                opened: jiff::Timestamp::UNIX_EPOCH,
                archived: false,
                locked: true,
            }])
        }
    }

    #[test]
    fn erased_markers_read_what_the_markers_they_erase_read() {
        let markers: &dyn DynMarkers = &Found;
        let key = Key::derive(
            &herald_core::RouteId::new("route"),
            &herald_core::Identity::new("event"),
        );
        let marker = Marker::of(&[1]);
        assert_eq!(
            pollster::block_on(Erased(markers).entry(&key, &marker)),
            Ok(Some(Entry::new(key)))
        );
        assert_eq!(
            pollster::block_on(Erased(markers).threads(&marker)),
            Ok(vec![MarkedThread {
                thread: "7".to_owned(),
                opened: jiff::Timestamp::UNIX_EPOCH,
                archived: false,
                locked: true,
            }])
        );
    }

    #[test]
    fn erased_secrets_read_as_the_secrets_they_erase() {
        let secrets: &dyn DynSecrets = &Named;
        assert_eq!(
            pollster::block_on(secrets.secret_boxed("K")),
            Ok(Secret::new("K", "value"))
        );
        assert_eq!(
            pollster::block_on(secrets.secret_boxed("")),
            Err(SecretError::Missing {
                name: String::new()
            })
        );
    }

    #[test]
    fn an_erased_port_answers_as_the_port_it_erases() {
        let http: &dyn DynHttp = &Echo;
        let request = Request::new(
            crate::http::Method::Get,
            crate::secret::Text::plain("https://example.org/a"),
        );
        let answered = pollster::block_on(Erased(http).send(request)).unwrap();
        assert_eq!(answered.body, b"https://example.org/a");
        let signer: &dyn DynSigner = &Reversed;
        let key = Secret::new("K", "k");
        assert_eq!(
            pollster::block_on(Erased(signer).sign(Algorithm::Ed25519, &key, b"abc")),
            Ok(b"cba".to_vec())
        );
        assert_eq!(
            pollster::block_on(Erased(signer).public(Algorithm::Ed25519, &key)),
            Ok(b"k".to_vec())
        );
        assert_eq!(
            pollster::block_on(Erased(signer).sign(
                Algorithm::Ed25519,
                &Secret::new("E", ""),
                b"x"
            )),
            Err(CryptoError::BadKey {
                name: "E".to_owned()
            })
        );
    }
}
